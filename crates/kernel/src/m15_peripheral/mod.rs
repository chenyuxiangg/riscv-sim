//! M-15 外设模型层 (Peripheral Model Layer).
//!
//! Adopt the "tiered peripheral" pattern (AD §4.3, M-15 v1.2):
//! - Each peripheral implements the [`Peripheral`] port from
//!   `riscv_sim_contract::port`.
//! - MVP ships three behaviour-level peripherals:
//!   [`Uart16550`], [`GpioBank`], [`TimerRiscv`].
//! - [`PeripheralRegistry`] is the registry that the kernel can
//!   consult at attach / map-peripheral time. The registry owns the
//!   instances and hands out [`SharedPeripheral`] handles
//!   (`BTreeMap<id, Rc<RefCell<dyn Peripheral>>>` keyed by the string id
//!   returned by `Peripheral::id()`). The handle is what M-14 keeps
//!   when a peripheral is mapped onto the bus (D-12) — the registry
//!   stays the owner, so the engine can still `tick` the instance and
//!   inspect its state.
//! - The three MVP peripherals override `Peripheral::on_probe` and
//!   report their state changes through the contract's `ProbeSink`
//!   (D-12 / N5): [`GpioBank`] reports `"OUT"`, [`Uart16550`] reports
//!   `"TX"`, [`TimerRiscv`] reports `"CMP"`. The signal names live
//!   next to the peripherals as `SIGNAL_*` constants so the M-18
//!   channel binding and the peripheral can never drift apart.
//!
//! Fidelity metadata ([`FidelityLevel`], per M-15 §2.1.1) is exposed
//! via [`PeripheralRegistry::describe`] for the front-end display
//! (评审 R5). All MVP peripherals are fidelity level = 1 (行为级).

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::port::ProbeSink;
use riscv_sim_contract::session::{M15PeriphDto, PeripheralSessionState};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::BTreeMap;
use std::rc::Rc;

pub mod gpio;
pub mod reg;
pub mod timer;
pub mod uart;

pub use gpio::GpioBank;
pub use timer::TimerRiscv;
pub use uart::Uart16550;
// M-15 review #10: re-export `Peripheral` directly so external code
// can write `m15_peripheral::Peripheral` (the previous `PeripheralTrait`
// alias forced callers back to `riscv_sim_contract::port::Peripheral`).
pub use riscv_sim_contract::port::Peripheral;

/// A shared handle to a peripheral instance.
///
/// M-14 (the bus) dispatches guest accesses into the instance while the
/// engine keeps a handle for `tick` / state inspection (M-14 LLD §3.2:
/// "命中外设转调 M-15 的 Peripheral.read/write" — the *instance* lives in
/// M-15, the bus only holds a reference). A shared handle is the Rust
/// spelling of that ownership split; the MVP is single-threaded, so
/// `Rc` (not `Arc`) is the right primitive — same posture as M-18's
/// `Rc<RefCell<ProbeRegistry>>`.
pub type SharedPeripheral = Rc<RefCell<dyn Peripheral>>;

/// Wrap a peripheral instance into a [`SharedPeripheral`] handle.
pub fn share<P: Peripheral + 'static>(p: P) -> SharedPeripheral {
    Rc::new(RefCell::new(p))
}

/// Holder for the M-18 probe callback of a peripheral (D-12).
///
/// The contract's `ProbeSink` is a bare closure with no `Debug` bound,
/// so the peripherals keep it in this wrapper to preserve their derived
/// `Debug` impls. `ProbeSlot` is the single place where "a peripheral
/// reports a signal observation" is implemented.
#[derive(Default)]
pub(crate) struct ProbeSlot(Option<ProbeSink>);

impl ProbeSlot {
    /// Install / replace the callback (`None` detaches it).
    pub(crate) fn set(&mut self, cb: Option<ProbeSink>) {
        self.0 = cb;
    }

    /// Report one observation: `(signal, value, now_ps)`.
    ///
    /// A detached slot is a no-op — an unobserved peripheral must not
    /// pay for the probe path.
    pub(crate) fn report(&mut self, signal: &str, value: u64, now_ps: u64) {
        if let Some(sink) = self.0.as_mut() {
            sink(signal, value, now_ps);
        }
    }
}

impl std::fmt::Debug for ProbeSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProbeSlot")
            .field("attached", &self.0.is_some())
            .finish()
    }
}

/// Fidelity level (M-15 §2.1.1) — coarse granularity for the
/// front-end "Fidelity" column (评审 R5). M-15 review #17: this is
/// the real carrier behind `describe()`; was previously a stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FidelityLevel {
    /// Behavioural: state-machine only, no timing.
    Behavioural = 1,
    /// Transactional: behavioural + bus-cycle accurate.
    Transactional = 2,
    /// Timing-approximate: transactional + bounded timing accuracy.
    TimingApprox = 3,
    /// Cycle-accurate: pin/cycle level.
    CycleAccurate = 4,
}

impl FidelityLevel {
    /// One-line label used by the front-end (M-15 review #17).
    pub fn label(self) -> &'static str {
        match self {
            FidelityLevel::Behavioural => "behavioural-level",
            FidelityLevel::Transactional => "transactional-level",
            FidelityLevel::TimingApprox => "timing-approx-level",
            FidelityLevel::CycleAccurate => "cycle-accurate-level",
        }
    }
}

/// Peripheral registry — keyed by the stable id returned by
/// [`Peripheral::id`]. The kernel can attach any registered instance
/// to the bus at attach time.
///
/// The registry **owns** the instances and hands out [`SharedPeripheral`]
/// handles (D-12): the bus dispatches guest accesses into the same
/// instance the engine can still `tick` and inspect.
#[derive(Default)]
pub struct PeripheralRegistry {
    by_id: BTreeMap<String, SharedPeripheral>,
}

/// Manual `Debug` impl because `SharedPeripheral` has no `Debug` bound.
impl std::fmt::Debug for PeripheralRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeripheralRegistry")
            .field("count", &self.by_id.len())
            .field("ids", &self.by_id.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl PeripheralRegistry {
    /// New empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a peripheral. Returns `Err(RegisterError::Duplicate)`
    /// if a peripheral with the same id is already registered
    /// (M-15 review #9: previously silently replaced the existing
    /// entry).
    pub fn register<P: Peripheral + 'static>(&mut self, p: P) -> Result<&mut Self, RegisterError> {
        let id = p.id().to_string();
        if self.by_id.contains_key(&id) {
            return Err(RegisterError::Duplicate(id));
        }
        let _ = self.by_id.insert(id, share(p));
        Ok(self)
    }

    /// Look up by id.
    pub fn get(&self, id: &str) -> Option<Ref<'_, dyn Peripheral + 'static>> {
        self.by_id.get(id).map(|b| b.borrow())
    }

    /// Look up by id (mut). The lifetime gymnastics pin the returned
    /// borrow to `self`, not to the `BTreeMap` entry.
    pub fn get_mut(&mut self, id: &str) -> Option<RefMut<'_, dyn Peripheral + 'static>> {
        self.by_id.get_mut(id).map(move |b| b.borrow_mut())
    }

    /// The **shared handle** to a registered peripheral.
    ///
    /// This is what the bus needs to dispatch guest accesses into the
    /// instance (D-12): the registry stays the owner, the bus holds a
    /// handle to the very same object.
    pub fn handle(&self, id: &str) -> Option<SharedPeripheral> {
        self.by_id.get(id).cloned()
    }

    /// Iterate all registered peripherals.
    pub fn iter(&self) -> impl Iterator<Item = Ref<'_, dyn Peripheral + 'static>> {
        self.by_id.values().map(|b| b.borrow())
    }

    /// Number of registered peripherals.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True when no peripherals are registered.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Compact one-line fidelity description used by the front-end
    /// "Fidelity" column (评审 R5, M-15 review #17).
    pub fn describe(&self, id: &str) -> Option<&'static str> {
        if self.get(id).is_some() {
            Some(FidelityLevel::Behavioural.label())
        } else {
            None
        }
    }

    // -----------------------------------------------------------------------
    // Session snapshot (STAGE-3 §3.3) — segment 4.
    // -----------------------------------------------------------------------

    /// Export the internal state of **every** registered peripheral —
    /// segment 4's producer (STAGE-3 §3.3).
    ///
    /// The states are grouped **by kind** (`uarts` / `gpios` / `timers`)
    /// and each group is ordered by **id**. A peripheral that does not
    /// implement [`Peripheral::session_state`] contributes nothing — it
    /// has no session state to carry (the contract's default is `None`).
    pub fn export_state(&self) -> M15PeriphDto {
        collect_session_state(&self.live_instances())
    }

    /// Restore the internal state of every registered peripheral —
    /// segment 4's consumer (STAGE-3 §3.3).
    ///
    /// ## Matching
    ///
    /// By **id** — the stable id the peripheral reports through
    /// [`Peripheral::id`], not its bus address (an address is a
    /// *placement*, the id is the *identity*).
    ///
    /// ## No silent skipping
    ///
    /// The payload's peripheral set and the live instances must
    /// **agree**, in both directions:
    ///
    /// - a payload entry whose id is **not** live → **error**
    ///   ([`E_PER_05_PERIPHERAL_SET_MISMATCH`]);
    /// - a live peripheral that **participates** in the session state
    ///   (its [`Peripheral::session_state`] is `Some`) but is **not**
    ///   described by the payload → **error** (same code).
    ///
    /// ⚠️ Skipping either side would leave the restored session
    /// **different** from the captured one — and *silently*: nobody
    /// would ever learn that a peripheral's state was dropped. A
    /// peripheral that does **not** participate in the snapshot (the
    /// contract's default `None`) is not affected: it simply has no
    /// state to restore.
    ///
    /// ## All checks run before anything is written
    ///
    /// A rejected payload leaves every peripheral **exactly as it was**
    /// — a half-applied restore is worse than a rejected one (STAGE-3
    /// §5).
    pub fn load_state(&mut self, state: &M15PeriphDto) -> Result<(), PlatformError> {
        restore_session_states(&self.live_instances(), state)
    }

    /// The registered instances, in the shape the free functions take.
    fn live_instances(&self) -> Vec<(u32, SharedPeripheral)> {
        self.by_id
            .values()
            .map(|handle| {
                // The borrow is released immediately — the restore path
                // takes a **mutable** borrow of the very same cells.
                let base = { handle.borrow().base_addr() };
                (base, handle.clone())
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Session snapshot — the bus-bound flavour (STAGE-3 §3.3).
// ---------------------------------------------------------------------------

/// Collect the session state of the **live instances bound on a bus**.
///
/// The kernel host binds its peripherals **directly onto the bus** (it
/// keeps no [`PeripheralRegistry`]), so the registry's
/// [`PeripheralRegistry::export_state`] is not reachable from there.
/// This free function is the host's entry point: it walks the
/// [`Bus::peripheral_instances`](crate::m14_bus::Bus::peripheral_instances)
/// list and produces the very same payload.
///
/// ⚠️ **Borrows do not overlap.** Each instance is borrowed for the
/// duration of one `session_state()` call and released immediately —
/// a borrow held across the loop would make the *restore* path panic
/// (a `borrow_mut` while a `borrow` is alive).
pub fn collect_session_state(instances: &[(u32, SharedPeripheral)]) -> M15PeriphDto {
    let mut dto = M15PeriphDto::default();
    for (_base, handle) in instances {
        let state = {
            let periph = handle.borrow();
            periph.session_state()
        };
        match state {
            Some(PeripheralSessionState::Uart(s)) => dto.uarts.push(s),
            Some(PeripheralSessionState::Gpio(s)) => dto.gpios.push(s),
            Some(PeripheralSessionState::Timer(s)) => dto.timers.push(s),
            // A peripheral without session state contributes nothing.
            None => {}
        }
    }
    // Each group is ordered by id — the payload is a **canonical**
    // form, not a snapshot of the registration order.
    dto.uarts.sort_by(|a, b| a.id.cmp(&b.id));
    dto.gpios.sort_by(|a, b| a.id.cmp(&b.id));
    dto.timers.sort_by(|a, b| a.id.cmp(&b.id));
    dto
}

/// Restore the session state onto the **live instances bound on a bus**.
///
/// The bus-bound twin of [`PeripheralRegistry::load_state`] — same
/// matching rules, same errors, same "all checks before any write"
/// discipline. See that method for the full contract.
pub fn restore_session_states(
    instances: &[(u32, SharedPeripheral)],
    state: &M15PeriphDto,
) -> Result<(), PlatformError> {
    // ---- Phase 1: index the live instances. Nothing is written yet. ----
    let mut live: Vec<LiveInstance> = Vec::with_capacity(instances.len());
    for (_base, handle) in instances {
        let (id, kind) = {
            // The borrow ends with this block on purpose: the restore
            // below takes a **mutable** borrow of the same cell, and
            // holding both at once would panic.
            let periph = handle.borrow();
            let kind = periph.session_state().as_ref().map(SessionKind::of);
            (periph.id().to_string(), kind)
        };
        live.push(LiveInstance {
            id,
            kind,
            handle: handle.clone(),
        });
    }

    // The payload, flattened into (id, kind, state).
    let mut entries: Vec<(String, SessionKind, PeripheralSessionState)> = Vec::new();
    for s in &state.uarts {
        entries.push((
            s.id.clone(),
            SessionKind::Uart,
            PeripheralSessionState::Uart(s.clone()),
        ));
    }
    for s in &state.gpios {
        entries.push((
            s.id.clone(),
            SessionKind::Gpio,
            PeripheralSessionState::Gpio(s.clone()),
        ));
    }
    for s in &state.timers {
        entries.push((
            s.id.clone(),
            SessionKind::Timer,
            PeripheralSessionState::Timer(s.clone()),
        ));
    }

    // ---- Phase 2: every id in the payload must be live. ----
    for (id, _kind, _state) in &entries {
        if !live.iter().any(|l| l.id == *id) {
            return Err(PlatformError::new(
                PERIPHERAL_FAMILY,
                E_PER_05_PERIPHERAL_SET_MISMATCH,
                format!("会话载荷里的外设 '{id}' 没有对应的活实例——外设集合对不上，不能静默跳过"),
            ));
        }
    }

    // ---- Phase 3: every live peripheral that participates must be described. ----
    for l in &live {
        // `None` = this peripheral has no session state at all (the
        // contract's default) — there is nothing to restore.
        let Some(kind) = l.kind else { continue };
        if !entries
            .iter()
            .any(|(id, k, _state)| *id == l.id && *k == kind)
        {
            return Err(PlatformError::new(
                PERIPHERAL_FAMILY,
                E_PER_05_PERIPHERAL_SET_MISMATCH,
                format!(
                    "活实例外设 '{}' 在会话载荷里没有对应的状态——外设集合对不上，不能静默跳过",
                    l.id
                ),
            ));
        }
    }

    // ---- Phase 4: apply. One instance at a time, no overlapping borrows. ----
    for (id, _kind, variant) in &entries {
        let target = live
            .iter()
            .find(|l| l.id == *id)
            .expect("phase 2 rejected every id without a live instance");
        let mut periph = target.handle.borrow_mut();
        periph.restore_session_state(variant)?;
    }
    Ok(())
}

/// A live peripheral, as the restore path sees it.
struct LiveInstance {
    /// The peripheral's own id (its identity, not its placement).
    id: String,
    /// The kind it reports — `None` when the peripheral does not
    /// participate in the session snapshot.
    kind: Option<SessionKind>,
    /// The live handle.
    handle: SharedPeripheral,
}

/// Which kind of peripheral a session state belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionKind {
    Uart,
    Gpio,
    Timer,
}

impl SessionKind {
    /// The kind carried by a session state.
    fn of(state: &PeripheralSessionState) -> Self {
        match state {
            PeripheralSessionState::Uart(_) => Self::Uart,
            PeripheralSessionState::Gpio(_) => Self::Gpio,
            PeripheralSessionState::Timer(_) => Self::Timer,
        }
    }

    /// Human-readable name, for the error messages.
    fn name(self) -> &'static str {
        match self {
            Self::Uart => "UART",
            Self::Gpio => "GPIO",
            Self::Timer => "定时器",
        }
    }
}

/// Check that a session state names **this** instance.
///
/// The id inside the payload is the peripheral's own — a state that
/// names a *different* peripheral was handed to the wrong instance
/// (or the payload was built by hand). Applying it would move one
/// peripheral's state into another: the kind of corruption that is
/// invisible until the firmware misbehaves.
pub(crate) fn check_same_id(expected: &str, got: &str) -> Result<(), PlatformError> {
    if expected == got {
        return Ok(());
    }
    Err(PlatformError::new(
        PERIPHERAL_FAMILY,
        E_PER_05_PERIPHERAL_SET_MISMATCH,
        format!("会话状态里的外设 id 是 '{got}'，但被交给了 '{expected}'"),
    ))
}

/// Build the "this peripheral cannot take that state" error.
///
/// Reuses the contract's default-implementation code (the trait's
/// `restore_session_state` default rejects with scenario 4) — the
/// scenario is the same one: *this peripheral does not accept this
/// session state*.
pub(crate) fn wrong_state_kind(
    periph_id: &str,
    expected: SessionKind,
    got: SessionKind,
) -> PlatformError {
    PlatformError::new(
        PERIPHERAL_FAMILY,
        E_PER_04_UNSUPPORTED_SESSION_STATE,
        format!(
            "外设 '{periph_id}' 是{}，却收到了{}的状态",
            expected.name(),
            got.name()
        ),
    )
}

/// Error returned by [`PeripheralRegistry::register`] when an id
/// collision is detected (M-15 review #9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// A peripheral with this id is already registered.
    Duplicate(String),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterError::Duplicate(id) => {
                write!(f, "peripheral id '{}' already registered", id)
            }
        }
    }
}

impl std::error::Error for RegisterError {}

/// Family used by errors raised from this module.
pub const PERIPHERAL_FAMILY: ErrorFamily = ErrorFamily::Peripheral;

// ⚠️ The two session-state scenario codes live in the **contract layer**
// (`riscv_sim_contract::port`), not here: `E_PER_04` is the code the
// contract's own default implementation returns, so the contract has to be
// able to name it. Defining a second copy of the same number here would
// put one code in two places — the moment one of them moved, the two
// would silently drift apart.
//
// - [`E_PER_04_UNSUPPORTED_SESSION_STATE`] — this peripheral does not
//   accept the session state it was handed (wrong kind / malformed).
// - [`E_PER_05_PERIPHERAL_SET_MISMATCH`] — the payload's peripheral id set
//   does not match the live instances, **in either direction** (the
//   payload names a peripheral that is not live, or a live peripheral that
//   participates in the session state is not described by the payload).
//   Skipping either side would leave the restored session **different**
//   from the captured one — and *silently* (STAGE-3 §3.3).
pub use riscv_sim_contract::port::{
    E_PER_04_UNSUPPORTED_SESSION_STATE, E_PER_05_PERIPHERAL_SET_MISMATCH,
};

#[cfg(test)]
mod tests;
