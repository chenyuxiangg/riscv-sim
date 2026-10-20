//! M-18 探针 (Probe) — double-source probe pipeline.
//!
//! Two source layers (M-18 v0.8 §1.1, 决策点 4):
//!
//! 1. **Bus trace** (底 / base layer): every `BusAccess` from M-14
//!    flows into a separate ring buffer for M-11 trace export
//!    (instruction coverage / memory-access log, FR-14). This stream
//!    is *not* subject to the channel-based edge judgment below; it
//!    is full-fidelity, drop-oldest on overflow.
//!
//! 2. **Peripheral signal probes** (面 / surface layer): each
//!    peripheral reports signal changes through the additive
//!    `Peripheral::on_probe` callback (M-18 v0.8 §18-2, contract
//!    `ProbeSink`). The registry routes the report to the
//!    appropriate channel, applies edge judgment (single-bit →
//!    rising/falling, multi-bit → value, pulse → always-emit), and
//!    accumulates edge events for the waveform (M-19 downstream).
//!
//! The two streams are independent (M-18 v0.8 §2.1.1: "traceStream
//! 独立成流供 M-11 导出，不并入波形通道"). Both are read-only
//! observations — M-18 never mutates simulation state.
//!
//! ## MVP wiring
//!
//! The three wiring points are all in place:
//!
//! - **M-14 → M-18** (bus trace): [`ProbeRegistry::make_bus_hook`]
//!   hands a `BusHook` to the bus; every access lands in the trace
//!   ring (for M-11 export).
//! - **M-15 → M-18** (peripheral signals):
//!   [`ProbeRegistry::attach_signal`] binds a peripheral's signal to
//!   a channel *and* installs the callback into the peripheral — one
//!   call, the M-18 LLD §1.3 `attachSignal(periph, ch)`.
//! - **M-18 → M-19** (waveform handoff): [`ProbeRegistry::set_edge_sink`]
//!   installs the downstream [`EdgeSink`](riscv_sim_contract::port::EdgeSink).
//!
//! The four default channel numbers (`CH_TIMER_CMP`, `CH_GPIO0`,
//! `CH_IRQ_PULSE`, `CH_KEY_STATE`) come from M-18 v0.8 §2.1.1;
//! `CH_UART_TX` is an additional channel for the UART's per-byte TX
//! event stream (the channel map is an open table, not a fixed array).
//!
//! ## Module layout
//!
//! ```text
//! m18_probe/
//! ├── mod.rs          — re-exports + module doc
//! ├── channel.rs      — ChannelDef, ChannelKind, classify()
//! ├── trace_stream.rs — TraceStream (ring buffer + drop counter)
//! ├── registry.rs     — ProbeRegistry (orchestrator + sample())
//! └── tests.rs        — unit tests (LLD §7.1)
//! ```

pub mod channel;
pub mod registry;
pub mod trace_stream;

pub use channel::{
    ChannelDef, ChannelKind, CH_GPIO0, CH_IRQ_PULSE, CH_KEY_STATE, CH_TIMER_CMP, CH_UART_TX,
};
pub use registry::{ProbeError, ProbeRegistry, SampleCfg, EDGE_STORE_CAP};
pub use trace_stream::{TraceStream, TRACE_CAP};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
