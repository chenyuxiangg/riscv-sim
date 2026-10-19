//! M-19 波形管道 (Waveform Pipeline).
//!
//! Consumes edge-encoded streams from the probe registry (M-18) and
//! pushes them incrementally to subscribers (front-end waveform view
//! or trace export). The pipeline never mutates the underlying
//! simulation state — it is strictly read-only.
//!
//! Status: stub — incremental push channel is TODO(MVP).

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::Waveform;

/// Channel that downstream subscribers (front-end, trace exporter)
/// poll. The kernel pushes deltas here, never the full state.
#[derive(Debug, Default)]
pub struct WaveformChannel {
    last_push: Option<Waveform>,
    push_seq: u64,
}

impl WaveformChannel {
    /// New empty channel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish a delta. Replaces the previous `last_push`; the
    /// downstream decides how to coalesce.
    pub fn push(&mut self, wf: Waveform) -> Result<(), PlatformError> {
        self.last_push = Some(wf);
        self.push_seq = self.push_seq.wrapping_add(1);
        Ok(())
    }

    /// Most recent push, with its monotonic sequence number.
    pub fn last(&self) -> Option<(u64, &Waveform)> {
        self.push_seq.checked_sub(1).zip(self.last_push.as_ref())
    }
}

/// Family used by errors raised from this module.
pub const WAVEFORM_FAMILY: ErrorFamily = ErrorFamily::Observability;
