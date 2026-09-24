use super::*;

#[test]
fn snapshot_commits_on_phase_and_survives_a_bare_ping() {
    let id = CString::new("com.example.pro").unwrap();
    let price = CString::new("34,99 €").unwrap();
    let title = CString::new("Pro").unwrap();
    let body = CString::new("Everything unlocked").unwrap();
    let null = std::ptr::null();

    unsafe {
        on_message(
            std::ptr::null_mut(),
            KIND_BEGIN,
            0,
            0,
            null,
            null,
            null,
            null,
        );
        on_message(
            std::ptr::null_mut(),
            KIND_PRODUCT,
            0,
            0,
            id.as_ptr(),
            price.as_ptr(),
            title.as_ptr(),
            body.as_ptr(),
        );
        assert!(StoreKitPurchases.state().products.is_empty());
        on_message(
            std::ptr::null_mut(),
            KIND_OWNED,
            0,
            0,
            id.as_ptr(),
            null,
            null,
            null,
        );
        on_message(
            std::ptr::null_mut(),
            KIND_PHASE,
            PHASE_READY,
            0,
            null,
            null,
            null,
            null,
        );
    }

    let state = StoreKitPurchases.state();
    assert_eq!(state.phase, StorePhase::Ready);
    assert_eq!(state.display_price("com.example.pro"), Some("34,99 €"));
    assert!(state.owns("com.example.pro"));

    unsafe {
        on_message(
            std::ptr::null_mut(),
            KIND_PHASE,
            PHASE_CONNECTING,
            0,
            null,
            null,
            null,
            null,
        );
    }
    let state = StoreKitPurchases.state();
    assert_eq!(state.phase, StorePhase::Connecting);
    assert_eq!(state.display_price("com.example.pro"), Some("34,99 €"));
    assert!(state.owns("com.example.pro"));
}

#[test]
fn events_queue_and_drain_in_order_and_are_bounded() {
    while StoreKitPurchases.take_event().is_some() {}
    let msg = CString::new("card declined").unwrap();
    let null = std::ptr::null();
    unsafe {
        on_message(
            std::ptr::null_mut(),
            KIND_EVENT,
            EVENT_CANCELLED,
            0,
            null,
            null,
            null,
            null,
        );
        on_message(
            std::ptr::null_mut(),
            KIND_EVENT,
            EVENT_FAILED,
            0,
            msg.as_ptr(),
            null,
            null,
            null,
        );
        on_message(
            std::ptr::null_mut(),
            KIND_EVENT,
            EVENT_RESTORED,
            3,
            null,
            null,
            null,
            null,
        );
    }
    assert_eq!(
        StoreKitPurchases.take_event(),
        Some(PurchaseEvent::Cancelled)
    );
    assert_eq!(
        StoreKitPurchases.take_event(),
        Some(PurchaseEvent::Failed("card declined".into()))
    );
    assert_eq!(
        StoreKitPurchases.take_event(),
        Some(PurchaseEvent::Restored { restored: 3 })
    );
    assert_eq!(StoreKitPurchases.take_event(), None);

    for _ in 0..100 {
        unsafe {
            on_message(
                std::ptr::null_mut(),
                KIND_EVENT,
                EVENT_PENDING,
                0,
                null,
                null,
                null,
                null,
            );
        }
    }
    let mut drained = 0;
    while StoreKitPurchases.take_event().is_some() {
        drained += 1;
    }
    assert_eq!(drained, 32);
}
