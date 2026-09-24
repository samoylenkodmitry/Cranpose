use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

#[test]
fn default_backend_sells_nothing_and_owns_nothing() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_purchases();
    let state = store_state();
    assert_eq!(state.phase, StorePhase::Unavailable);
    assert!(state.owned.is_empty());
    assert!(!state.owns("com.example.pro"));
    assert!(!store_available());
    configure(&["com.example.pro"]);
    purchase("com.example.pro");
    restore();
    assert_eq!(take_event(), None);
}

#[test]
fn the_two_phases_that_cannot_sell_differ_on_whether_waiting_helps() {
    assert!(StorePhase::Unavailable.cannot_sell());
    assert!(StorePhase::Blocked.cannot_sell());
    assert!(!StorePhase::Connecting.cannot_sell());
    assert!(!StorePhase::Ready.cannot_sell());

    assert!(StorePhase::Unavailable.may_yet_change());
    assert!(StorePhase::Connecting.may_yet_change());
    assert!(
        !StorePhase::Blocked.may_yet_change(),
        "a store that has said no is what the phase exists to say"
    );
    assert!(!StorePhase::Ready.may_yet_change());
}

#[test]
fn nothing_is_owned_by_default_and_blocked_is_not_the_default() {
    assert_eq!(StorePhase::default(), StorePhase::Unavailable);
}

#[test]
fn installed_backend_answers_prices_and_ownership() {
    let _guard = crate::registry::test_service_guard();
    struct Fake;
    impl Purchases for Fake {
        fn configure(&self, _product_ids: &[&str]) {}
        fn state(&self) -> StoreState {
            StoreState {
                phase: StorePhase::Ready,
                products: vec![Product {
                    id: "com.example.pro".into(),
                    display_price: "34,99 €".into(),
                    title: "Pro".into(),
                    description: "Everything unlocked".into(),
                }],
                owned: BTreeSet::from(["com.example.pro".to_string()]),
                orders: BTreeMap::from([(
                    "com.example.pro".to_string(),
                    "GPA.1234-5678".to_string(),
                )]),
                error: None,
                busy: false,
            }
        }
        fn purchase(&self, _product_id: &str) {}
        fn restore(&self) {}
        fn take_event(&self) -> Option<PurchaseEvent> {
            Some(PurchaseEvent::Purchased("com.example.pro".into()))
        }
        fn is_connected(&self) -> bool {
            true
        }
        fn reconnect(&self) {}
    }
    set_platform_purchases(Arc::new(Fake));
    let state = store_state();
    assert_eq!(state.phase, StorePhase::Ready);
    assert!(state.owns("com.example.pro"));
    assert_eq!(state.order_id("com.example.pro"), Some("GPA.1234-5678"));
    assert_eq!(state.order_id("com.example.free"), None);
    assert_eq!(state.display_price("com.example.pro"), Some("34,99 €"));
    assert_eq!(state.display_price("com.example.nope"), None);
    assert!(store_available());
    assert_eq!(
        take_event(),
        Some(PurchaseEvent::Purchased("com.example.pro".into()))
    );
    clear_platform_purchases();
}

#[test]
fn dead_store_reconnects_before_frame_state_is_read() {
    let _guard = crate::registry::test_service_guard();
    struct Reconnecting {
        alive: AtomicBool,
        reconnects: AtomicUsize,
    }
    impl Purchases for Reconnecting {
        fn configure(&self, _product_ids: &[&str]) {}
        fn state(&self) -> StoreState {
            StoreState {
                phase: if self.alive.load(Ordering::Acquire) {
                    StorePhase::Ready
                } else {
                    StorePhase::Unavailable
                },
                ..StoreState::default()
            }
        }
        fn purchase(&self, _product_id: &str) {}
        fn restore(&self) {}
        fn take_event(&self) -> Option<PurchaseEvent> {
            None
        }
        fn is_connected(&self) -> bool {
            self.alive.load(Ordering::Acquire)
        }
        fn reconnect(&self) {
            self.reconnects.fetch_add(1, Ordering::AcqRel);
            self.alive.store(true, Ordering::Release);
        }
    }
    clear_platform_purchases();
    let purchases = Arc::new(Reconnecting {
        alive: AtomicBool::new(false),
        reconnects: AtomicUsize::new(0),
    });
    set_platform_purchases(purchases.clone());
    assert_eq!(store_state().phase, StorePhase::Ready);
    assert_eq!(purchases.reconnects.load(Ordering::Acquire), 1);
    clear_platform_purchases();
}

#[test]
fn store_observers_receive_news_until_dropped() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let observer = observe_store_news(move || {
        seen.fetch_add(1, Ordering::Relaxed);
    });
    note_store_news();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(observer);
    note_store_news();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}
