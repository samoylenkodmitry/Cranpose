use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

#[test]
fn registration_round_trips() {
    let _services = crate::registry::test_service_guard();
    clear_platform_background_activity();
    struct Rec(AtomicBool);
    impl BackgroundActivity for Rec {
        fn set_active(&self, active: bool) {
            self.0.store(active, Ordering::SeqCst);
        }
    }
    let rec = Arc::new(Rec(AtomicBool::new(false)));
    set_platform_background_activity(rec.clone());
    let first = acquire_background_work();
    assert!(rec.0.load(Ordering::SeqCst));
    assert!(background_active());
    let second = acquire_background_work();
    drop(first);
    assert!(background_active());
    assert!(rec.0.load(Ordering::SeqCst));
    drop(second);
    assert!(!background_active());
    assert!(!rec.0.load(Ordering::SeqCst));
    clear_platform_background_activity();
}
