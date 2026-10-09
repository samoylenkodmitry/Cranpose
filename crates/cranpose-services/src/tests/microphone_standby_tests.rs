use std::sync::Arc;

use parking_lot::Mutex;

use super::*;

#[derive(Default)]
struct Recorded(Mutex<Vec<bool>>);

impl MicrophoneStandby for Recorded {
    fn set_held(&self, held: bool) {
        self.0.lock().push(held);
    }
}

#[test]
fn the_standby_runs_from_the_first_lease_to_the_last() {
    let _guard = crate::registry::test_service_guard();
    let recorded = Arc::new(Recorded::default());
    set_platform_microphone_standby(Some(recorded.clone()));
    assert!(microphone_standby_available());

    let first = hold_microphone_standby();
    let second = hold_microphone_standby();
    assert_eq!(*recorded.0.lock(), vec![true]);
    drop(first);
    assert_eq!(*recorded.0.lock(), vec![true]);
    drop(second);
    assert_eq!(*recorded.0.lock(), vec![true, false]);
    set_platform_microphone_standby(None);
}

#[test]
fn a_lease_without_a_platform_holds_nothing() {
    let _guard = crate::registry::test_service_guard();
    set_platform_microphone_standby(None);
    assert!(!microphone_standby_available());
    drop(hold_microphone_standby());
}
