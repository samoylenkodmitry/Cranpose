use std::sync::Arc;

use parking_lot::Mutex;

use super::*;

#[derive(Default)]
struct Recorded(Mutex<Vec<bool>>, Mutex<Vec<StandbyNotice>>);

impl MicrophoneStandby for Recorded {
    fn set_held(&self, held: bool) {
        self.0.lock().push(held);
    }

    fn set_notice(&self, notice: &StandbyNotice) {
        self.1.lock().push(notice.clone());
    }
}

#[test]
fn the_standby_runs_from_the_first_lease_to_the_last() {
    let _guard = crate::registry::test_service_guard();
    let recorded = Arc::new(Recorded::default());
    set_platform_microphone_standby(Some(Arc::<Recorded>::clone(&recorded)));
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

#[test]
fn the_notice_reaches_the_platform_and_its_stop_reaches_the_application() {
    let _guard = crate::registry::test_service_guard();
    let recorded = Arc::new(Recorded::default());
    set_platform_microphone_standby(Some(Arc::<Recorded>::clone(&recorded)));
    let notice = StandbyNotice {
        text: "Recording".into(),
        stop: "Stop".into(),
    };
    set_microphone_standby_notice(notice.clone());
    assert_eq!(*recorded.1.lock(), vec![notice]);

    let stops = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&stops);
    on_microphone_standby_stop(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    microphone_standby_stopped();
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    set_platform_microphone_standby(None);
}
