//! M-12 事件驱动仿真引擎 (Event-Driven Simulation Engine).
//!
//! Implements the quantumized hybrid scheduler (AD ADR-1, LLD §4.12):
//! - Time quantum: 1µs by default (`TIME_QUANTUM_PS`).
//! - At each quantum boundary, drain pending interrupts, run pending
//!   peripheral ticks, then let the ISS execute a slice of instructions.
//! - Pending writes from a quantum are buffered and committed at the
//!   end of the quantum — this is what makes the engine deterministic
//!   across replays.
//!
//! TODO(MVP): wire the scheduler to the [`crate::m13_iss`] decoder, the
//! peripheral tick callbacks, and the snapshot / restore path. The
//! current skeleton compiles and exposes the right port surface; the
//! inner event-queue implementation is a placeholder.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, SimState};
use riscv_sim_contract::port::{EventBus, InterruptSink};
use riscv_sim_contract::protocol::EventPayload;

/// Default time quantum, in picoseconds. Configurable per-project (M-12
/// §4.12.2).
pub const TIME_QUANTUM_PS: u64 = 1_000; // 1 µs

/// Default guest-instruction budget before the engine yields to the host.
/// Implemented as the watchdog pre-check (M-12 §5.2 + LLD §4.12.6).
pub const DEFAULT_INSTRUCTION_BUDGET: u32 = 10_000;

/// Scheduling event held in the event queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedEvent {
    /// A peripheral asks to be ticked at the given virtual time.
    PeripheralTick { paddr: u32, at_ps: u64 },
    /// A timer expiry (M-15 internal timers).
    TimerExpiry { irq: u16, at_ps: u64 },
    /// A pending memory write to commit at the next quantum boundary.
    Commit(PendingWrite),
}

impl Default for SchedEvent {
    fn default() -> Self {
        Self::TimerExpiry { irq: 0, at_ps: 0 }
    }
}

/// The event-driven engine itself.
#[derive(Debug)]
pub struct Engine<B: EventBus, I: InterruptSink> {
    /// Monotonic virtual clock.
    now_ps: u64,
    /// Pending events, time-ordered.
    queue: std::collections::BinaryHeap<QueuedEvent>,
    /// Instruction budget for the current quantum. Reserved for the
    /// watchdog pre-check (M-12 §5.2 + LLD §4.12.6); not yet threaded
    /// through `dispatch` in the MVP skeleton.
    #[allow(dead_code)]
    instruction_budget: u32,
    /// Bus / event / interrupt sinks.
    bus: B,
    intc: I,
    /// Cached `SimState` so the engine can snapshot / restore (M-20).
    state: SimState,
}

/// Priority queue item (min-heap on `at_ps`, but `BinaryHeap` is a
/// max-heap so we invert the ordering).
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedEvent {
    at_ps: u64,
    seq: u64,
    event: SchedEvent,
}

impl PartialOrd for QueuedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Reverse: earliest first. Non-canonical: the `Ord` impl flips
        // ordering so BinaryHeap acts as a min-heap.
        // (Lint explicitly allowed at the impl block.)
        other.at_ps.partial_cmp(&self.at_ps)
    }
}
impl Ord for QueuedEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .at_ps
            .cmp(&self.at_ps)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl<B: EventBus, I: InterruptSink> Engine<B, I> {
    /// Construct a fresh engine.
    pub fn new(bus: B, intc: I) -> Self {
        Self {
            now_ps: 0,
            queue: std::collections::BinaryHeap::new(),
            instruction_budget: DEFAULT_INSTRUCTION_BUDGET,
            bus,
            intc,
            state: SimState::default(),
        }
    }

    /// Current virtual time in picoseconds.
    pub fn now_ps(&self) -> u64 {
        self.now_ps
    }

    /// Schedule an event for the given virtual time.
    pub fn schedule(&mut self, at_ps: u64, event: SchedEvent) {
        self.queue.push(QueuedEvent {
            at_ps,
            seq: self.state.event_queue_head,
            event,
        });
        self.state.event_queue_head = self.state.event_queue_head.wrapping_add(1);
    }

    /// Run the engine until the queue is empty or a host pause is
    /// requested. Returns the number of quanta executed.
    ///
    /// Each quantum: advance virtual time by [`TIME_QUANTUM_PS`],
    /// publish a [`EventPayload::Tick`], then drain every event whose
    /// `at_ps` ≤ the new `now_ps`. An event scheduled exactly on a
    /// quantum boundary is dispatched in the quantum that ends at that
    /// boundary (so a 2-quantum run drains a single event at `t = 2 *
    /// TIME_QUANTUM_PS`).
    pub fn run_until_idle(&mut self) -> Result<u64, PlatformError> {
        let mut quanta: u64 = 0;
        while self.queue.peek().is_some() {
            // Advance the clock by exactly one quantum.
            self.now_ps = self.now_ps.wrapping_add(TIME_QUANTUM_PS);
            self.bus
                .publish(EventPayload::Tick {
                    elapsed_ps: self.now_ps,
                    pc: self.state.pc,
                })
                .map_err(|e| PlatformError::new(ErrorFamily::Engine, 1, e.to_string()))?;
            quanta = quanta.wrapping_add(1);

            // Drain every event that is now due.
            while let Some(t) = self.queue.peek().map(|e| e.at_ps) {
                if t > self.now_ps {
                    break;
                }
                let evt = self.queue.pop().expect("peek said some");
                self.dispatch(evt.event)?;
            }
        }
        Ok(quanta)
    }

    fn dispatch(&mut self, event: SchedEvent) -> Result<(), PlatformError> {
        match event {
            SchedEvent::PeripheralTick { paddr: _, at_ps: _ } => {
                // TODO(MVP): look up the peripheral registered at paddr
                // and call its `tick` port.
                Ok(())
            }
            SchedEvent::TimerExpiry { irq, at_ps: _ } => self.intc.raise(irq),
            SchedEvent::Commit(write) => {
                // Commit happens at the next bus access; for the
                // skeleton we just stash it on the state.
                self.state.pending_writes.push(write);
                Ok(())
            }
        }
    }

    /// Snapshot the deterministic engine state (M-20 §4.20).
    pub fn snapshot(&self) -> SimState {
        self.state.clone()
    }

    /// Restore from a snapshot.
    pub fn restore(&mut self, state: SimState) {
        self.state = state;
    }
}

impl<B: EventBus, I: InterruptSink> Default for Engine<B, I>
where
    B: Default,
    I: Default,
{
    fn default() -> Self {
        Self::new(B::default(), I::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal in-memory event bus for tests.
    #[derive(Default)]
    struct TestBus {
        count: u32,
    }
    impl EventBus for TestBus {
        fn publish(&mut self, _: EventPayload) -> Result<(), PlatformError> {
            self.count += 1;
            Ok(())
        }
    }
    impl InterruptSink for TestBus {
        fn raise(&mut self, _: u16) -> Result<(), PlatformError> {
            Ok(())
        }
        fn lower(&mut self, _: u16) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    #[test]
    fn engine_advances_time() {
        let mut eng: Engine<TestBus, TestBus> = Engine::default();
        eng.schedule(
            2 * TIME_QUANTUM_PS,
            SchedEvent::TimerExpiry { irq: 1, at_ps: 0 },
        );
        let quanta = eng.run_until_idle().expect("run");
        assert_eq!(quanta, 2);
        assert!(eng.now_ps() >= 2 * TIME_QUANTUM_PS);
    }
}
