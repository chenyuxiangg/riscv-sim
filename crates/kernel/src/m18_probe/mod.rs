//! M-18 探针 (Probe).
//!
//! The probe pipeline attaches to bus signals and peripheral outputs
//! to produce the edge-encoded waveform stream (M-19). This module
//! owns the [`Probe`](riscv_sim_contract::port::Probe) trait and a
//! default in-memory edge store.
//!
//! Status: stub — public port adopted; no concrete probe sources yet.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{Edge, Waveform, WaveformStream};
use riscv_sim_contract::port::Probe;
use std::collections::BTreeMap;

/// In-memory edge store for a single probed signal.
#[derive(Debug, Default)]
pub struct InMemoryProbe {
    id: String,
    label: String,
    edges: Vec<Edge>,
}

impl InMemoryProbe {
    /// New probe with the given id and human-readable label.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), edges: Vec::new() }
    }

    /// Snapshot the probe's edges as a [`WaveformStream`].
    pub fn as_stream(&self) -> WaveformStream {
        WaveformStream { id: self.id.clone(), label: self.label.clone(), edges: self.edges.clone() }
    }
}

impl Probe for InMemoryProbe {
    fn id(&self) -> &str { &self.id }
    fn observe(&mut self, now_ps: u64, value: u64) -> Result<(), PlatformError> {
        if let Some(last) = self.edges.last_mut() {
            if last.t_ps == now_ps {
                last.value = value;
                return Ok(());
            }
            if last.value == value {
                // Collapse duplicates: same value as the most recent edge.
                return Ok(());
            }
        }
        self.edges.push(Edge { t_ps: now_ps, value });
        Ok(())
    }
    fn drain(&mut self) -> Result<Vec<Edge>, PlatformError> {
        Ok(std::mem::take(&mut self.edges))
    }
}

/// Registry of probes keyed by id.
#[derive(Debug, Default)]
pub struct ProbeRegistry {
    probes: BTreeMap<String, InMemoryProbe>,
}

impl ProbeRegistry {
    /// New empty registry.
    pub fn new() -> Self { Self::default() }

    /// Register a probe.
    pub fn register(&mut self, p: InMemoryProbe) {
        let _ = self.probes.insert(p.id.clone(), p);
    }

    /// Take a snapshot of all probes' edges as a [`Waveform`].
    pub fn drain_waveform(&mut self) -> Result<Waveform, PlatformError> {
        let mut streams = Vec::with_capacity(self.probes.len());
        for p in self.probes.values_mut() {
            let id = p.id.clone();
            let label = p.label.clone();
            let edges = p.drain()?;
            streams.push(WaveformStream { id, label, edges });
        }
        Ok(Waveform { schema_version: 1, streams })
    }
}

/// Family used by errors raised from this module.
pub const PROBE_FAMILY: ErrorFamily = ErrorFamily::Observability;
