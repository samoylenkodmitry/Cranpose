use std::{thread, time::Duration};

use super::RecoveryGate;

#[test]
fn recovery_gate_retries_after_backoff_and_resets() {
    let gate = RecoveryGate::new();
    assert!(gate.try_start());
    assert!(!gate.try_start());
    thread::sleep(Duration::from_millis(20));
    assert!(gate.try_start());
    gate.succeeded();
    assert!(gate.try_start());
}
