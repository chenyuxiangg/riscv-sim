//! M-16 unit tests — the session-snapshot segment 5 (STAGE-3 §3.4).
//!
//! Coverage: the raised-IRQ-line set exported / restored as segment 5
//! of the session payload.

use super::*;
use riscv_sim_contract::session::M16IntcDto;

#[test]
fn export_state_lists_the_raised_lines_in_ascending_order() {
    let mut intc = InterruptController::new();
    intc.raise(5).expect("raise");
    intc.raise(1).expect("raise");
    intc.raise(3).expect("raise");

    assert_eq!(
        intc.export_state().raised,
        vec![1, 3, 5],
        "the raised lines are exported ascending"
    );
}

#[test]
fn export_state_is_empty_when_nothing_is_raised() {
    let intc = InterruptController::new();
    assert!(
        intc.export_state().raised.is_empty(),
        "nothing raised → an empty segment"
    );
}

#[test]
fn load_state_replaces_the_raised_lines() {
    // The payload *describes* the interrupt state: a line that was
    // raised before the snapshot and is not in the payload must not
    // survive the restore.
    let mut intc = InterruptController::new();
    intc.raise(9).expect("raise");

    intc.load_state(&M16IntcDto {
        raised: vec![2, 4],
        ..M16IntcDto::default()
    });

    assert_eq!(intc.export_state().raised, vec![2, 4]);
    assert_eq!(
        intc.drain_pending(),
        vec![2, 4],
        "the restored lines are the ones the engine samples"
    );
}

#[test]
fn load_state_keeps_the_set_canonical() {
    // The set is canonical: duplicates collapse and the order is
    // ascending, whatever order the payload used.
    let mut intc = InterruptController::new();
    intc.load_state(&M16IntcDto {
        raised: vec![9, 3, 3, 1],
        ..M16IntcDto::default()
    });
    assert_eq!(intc.export_state().raised, vec![1, 3, 9]);
}

#[test]
fn export_then_load_round_trips_the_raised_lines() {
    let mut intc = InterruptController::new();
    intc.raise(7).expect("raise");
    intc.raise(2).expect("raise");
    let state = intc.export_state();

    let mut fresh = InterruptController::new();
    fresh.load_state(&state);

    assert_eq!(fresh.export_state(), state);
    assert_eq!(fresh.drain_pending(), vec![2, 7]);
}

#[test]
fn load_state_clears_the_lines_when_the_payload_is_empty() {
    let mut intc = InterruptController::new();
    intc.raise(3).expect("raise");
    intc.load_state(&M16IntcDto {
        raised: Vec::new(),
        ..M16IntcDto::default()
    });
    assert!(
        intc.export_state().raised.is_empty(),
        "an empty payload means no line is raised"
    );
}
