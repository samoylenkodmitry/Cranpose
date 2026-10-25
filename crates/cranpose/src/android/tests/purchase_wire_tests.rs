use super::*;

#[test]
fn an_owned_row_carries_the_order_id_when_the_store_had_one() {
    let state = decode_store_snapshot(concat!(
        "2\t0\t\n",
        "o\tcom.example.pro\tGPA.3311-9944-1234-56789\n",
        "o\tcom.example.hints"
    ));

    assert!(state.owns("com.example.pro"));
    assert_eq!(
        state.order_id("com.example.pro"),
        Some("GPA.3311-9944-1234-56789")
    );

    assert!(
        state.owns("com.example.hints"),
        "a row without an order id is a normal owned product"
    );
    assert_eq!(state.order_id("com.example.hints"), None);
}

#[test]
fn an_order_id_containing_a_tab_survives_the_wire() {
    let state = decode_store_snapshot(concat!("2\t0\t\n", "o\tcom.example.pro\tGPA%09odd"));
    assert_eq!(state.order_id("com.example.pro"), Some("GPA\todd"));
}

#[test]
fn decoding_recovers_prices_and_owned_entitlements() {
    let state = decode_store_snapshot(concat!(
        "2\t0\t\n",
        "p\tcom.example.pro\t34,99 €\tPro\tEverything unlocked\n",
        "p\tcom.example.hints\t0,99 €\tHints\tA nudge\n",
        "o\tcom.example.pro"
    ));

    assert_eq!(state.phase, StorePhase::Ready);
    assert!(!state.busy);
    assert_eq!(state.error, None);
    assert_eq!(state.display_price("com.example.pro"), Some("34,99 €"));
    assert_eq!(
        state.product("com.example.pro").map(|p| p.title.as_str()),
        Some("Pro")
    );
    assert!(state.owns("com.example.pro"));
    assert!(!state.owns("com.example.hints"));
}

#[test]
fn decoding_reports_a_busy_connection_and_its_error() {
    let state = decode_store_snapshot("1\t1\tPlay Store not reached");

    assert_eq!(state.phase, StorePhase::Connecting);
    assert!(state.busy);
    assert_eq!(state.error.as_deref(), Some("Play Store not reached"));
    assert!(state.products.is_empty());
    assert!(state.owned.is_empty());
}

#[test]
fn decoding_restores_escaped_prices_and_descriptions() {
    let state = decode_store_snapshot(concat!(
        "2\t0\tone%0Atwo%25three\n",
        "p\tcom.example.pro\t%2534,99\tPro%09Plus\tTwo%0Alines"
    ));

    assert_eq!(state.error.as_deref(), Some("one\ntwo%three"));
    assert_eq!(state.display_price("com.example.pro"), Some("%34,99"));
    let product = state.product("com.example.pro").expect("product decoded");
    assert_eq!(product.title, "Pro\tPlus");
    assert_eq!(product.description, "Two\nlines");
}

#[test]
fn decoding_skips_records_it_cannot_read() {
    let state = decode_store_snapshot(concat!(
        "2\t0\t\n",
        "x\tsubscription\t?\n",
        "p\n",
        "p\t\t1,00 €\n",
        "p\tcom.example.nameless\n",
        "o\n",
        "o\t\n",
        "p\tcom.example.pro\t34,99 €\n",
        "o\tcom.example.pro"
    ));

    assert_eq!(state.products.len(), 1);
    assert_eq!(state.display_price("com.example.pro"), Some("34,99 €"));
    assert_eq!(
        state.product("com.example.pro").map(|p| p.title.len()),
        Some(0)
    );
    assert_eq!(state.owned.len(), 1);
}

#[test]
fn an_unreadable_payload_owns_nothing() {
    for payload in ["", "\n", "nonsense", "9\t1\t", "o\tcom.example.pro"] {
        let state = decode_store_snapshot(payload);
        assert_eq!(
            state.phase,
            StorePhase::Unavailable,
            "payload {payload:?} must not report a live store"
        );
        assert!(
            !state.owns("com.example.pro"),
            "payload {payload:?} must not grant an entitlement"
        );
    }
}

#[test]
fn a_store_that_will_not_sell_here_is_told_apart_from_one_not_reached() {
    let blocked = decode_store_snapshot("3\t0\tBILLING_UNAVAILABLE");
    assert_eq!(blocked.phase, StorePhase::Blocked);
    assert!(blocked.phase.cannot_sell());
    assert!(
        !blocked.phase.may_yet_change(),
        "a store that has said no is not worth waiting on"
    );

    let unreached = decode_store_snapshot("0\t0\tSERVICE_UNAVAILABLE");
    assert_eq!(unreached.phase, StorePhase::Unavailable);
    assert!(unreached.phase.cannot_sell());
    assert!(
        unreached.phase.may_yet_change(),
        "a store that was merely not reached may answer on the next try"
    );
}

#[test]
fn a_blocked_store_still_reports_what_the_account_owns() {
    let state = decode_store_snapshot("3\t0\t\no\tcom.example.pro");
    assert_eq!(state.phase, StorePhase::Blocked);
    assert!(state.owns("com.example.pro"));
}

#[test]
fn events_carry_the_product_the_message_and_the_restore_count() {
    assert_eq!(
        decode_purchase_event(EVENT_PURCHASED, "com.example.pro".into(), 0),
        Some(PurchaseEvent::Purchased("com.example.pro".into()))
    );
    assert_eq!(
        decode_purchase_event(EVENT_CANCELLED, String::new(), 0),
        Some(PurchaseEvent::Cancelled)
    );
    assert_eq!(
        decode_purchase_event(EVENT_PENDING, String::new(), 0),
        Some(PurchaseEvent::Pending)
    );
    assert_eq!(
        decode_purchase_event(EVENT_FAILED, "card declined".into(), 0),
        Some(PurchaseEvent::Failed("card declined".into()))
    );
    assert_eq!(
        decode_purchase_event(EVENT_RESTORED, String::new(), 3),
        Some(PurchaseEvent::Restored { restored: 3 })
    );
    assert_eq!(decode_purchase_event(99, String::new(), 0), None);
}

#[test]
fn a_failure_without_a_reason_still_has_something_to_show() {
    let Some(PurchaseEvent::Failed(message)) =
        decode_purchase_event(EVENT_FAILED, String::new(), 0)
    else {
        panic!("a failure event should decode");
    };
    assert!(!message.is_empty());
}

#[test]
fn a_negative_restore_count_is_read_as_none_found() {
    assert_eq!(
        decode_purchase_event(EVENT_RESTORED, String::new(), -1),
        Some(PurchaseEvent::Restored { restored: 0 })
    );
}
