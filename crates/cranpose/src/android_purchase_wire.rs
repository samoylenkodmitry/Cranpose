//! The wire format that carries Google Play Billing state into
//! [`cranpose_services::purchases`].
//!
//! `CranposeBilling` flattens everything the billing client knows —
//! connection phase, localized products, owned entitlements — into one string,
//! which crosses JNI as a single `String` rather than one call per product.
//! Decoding lives here, in safe Rust, and is unit-tested on the host — the
//! format is the contract between the two sides.
//!
//! ```text
//! <phase>\t<busy>\t<error>              header, always the first line
//! p\t<id>\t<price>\t<title>\t<body>     one record per product the store knows
//! o\t<id>                               one record per owned entitlement
//! ```
//!
//! `phase` is `0`, `1`, `2` or `3` for unavailable, connecting, ready and
//! blocked — the last being a store that answered and will not sell; `busy` is
//! `1` while a purchase or restore the user asked for is still running. Names
//! and values use the same `%09`/`%0A`/`%0D`/`%25` escaping as the
//! launch-argument payload, because a store-authored title, description or
//! formatted price may contain anything.
//!
//! **Java sends the whole snapshot every time.** The billing client is the one
//! that knows what it has learned so far, so a reader can never observe a
//! half-built product list, and a "connecting" ping cannot blank prices the
//! user is already looking at — the decode either replaces the state or the
//! payload is dropped. Unknown record types and malformed records are skipped,
//! so a Java side that learns to describe more of a product does not have to
//! be decoded in lock step.

use cranpose_services::purchases::{Product, PurchaseEvent, StorePhase, StoreState};
use std::collections::{BTreeMap, BTreeSet};

/// One-shot event codes, mirroring the constants in `CranposeBilling.java`.
/// Both sides live in this repository and change together.
pub(crate) const EVENT_PURCHASED: i32 = 0;
pub(crate) const EVENT_CANCELLED: i32 = 1;
pub(crate) const EVENT_PENDING: i32 = 2;
pub(crate) const EVENT_FAILED: i32 = 3;
pub(crate) const EVENT_RESTORED: i32 = 4;

/// Decodes a payload produced by `CranposeBilling.pushSnapshot`.
pub(crate) fn decode_store_snapshot(payload: &str) -> StoreState {
    let mut lines = payload.split('\n');
    let mut header = lines.next().unwrap_or_default().split('\t');
    let phase = match header.next() {
        Some("1") => StorePhase::Connecting,
        Some("2") => StorePhase::Ready,
        Some("3") => StorePhase::Blocked,
        // Anything the decoder cannot read is "no store": refusing to guess is
        // what keeps a garbled payload from granting an entitlement. It is
        // also the safer of the two unsellable phases to guess — it invites a
        // retry, where `Blocked` tells the user to stop trying.
        _ => StorePhase::Unavailable,
    };
    let busy = matches!(header.next(), Some("1"));
    let error = header.next().map(unescape).filter(|text| !text.is_empty());

    let mut products = Vec::new();
    let mut owned = BTreeSet::new();
    let mut orders = BTreeMap::new();
    for record in lines {
        let mut fields = record.split('\t');
        match fields.next() {
            Some("p") => products.extend(decode_product(&mut fields)),
            Some("o") => {
                if let Some((id, order)) = decode_owned(&mut fields) {
                    if let Some(order) = order {
                        orders.insert(id.clone(), order);
                    }
                    owned.insert(id);
                }
            }
            _ => {}
        }
    }

    StoreState {
        phase,
        products,
        owned,
        orders,
        error,
        busy,
    }
}

/// Turns an event code from `nativeBillingEvent` into the API's own event.
///
/// `message` carries the purchased product id for [`PurchaseEvent::Purchased`]
/// and a user-facing sentence for [`PurchaseEvent::Failed`]; `count` is the
/// number of entitlements a restore turned up.
pub(crate) fn decode_purchase_event(
    code: i32,
    message: String,
    count: i32,
) -> Option<PurchaseEvent> {
    match code {
        EVENT_PURCHASED => Some(PurchaseEvent::Purchased(message)),
        EVENT_CANCELLED => Some(PurchaseEvent::Cancelled),
        EVENT_PENDING => Some(PurchaseEvent::Pending),
        EVENT_FAILED => Some(PurchaseEvent::Failed(if message.is_empty() {
            // The store did not say why. The string is shown to the user, so
            // it cannot be left blank.
            "The purchase could not be completed".to_string()
        } else {
            message
        })),
        EVENT_RESTORED => Some(PurchaseEvent::Restored {
            restored: count.max(0) as usize,
        }),
        _ => None,
    }
}

fn decode_product<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Option<Product> {
    let id = unescape(fields.next()?);
    if id.is_empty() {
        return None;
    }
    Some(Product {
        id,
        display_price: unescape(fields.next()?),
        // A store is free to leave these empty; a product without a price is
        // one the app cannot sell, which is why only the price is required.
        title: fields.next().map(unescape).unwrap_or_default(),
        description: fields.next().map(unescape).unwrap_or_default(),
    })
}

/// An owned record: the product id, and the order id if the store had one.
///
/// The order id is OPTIONAL and always has been on the wire -- Play does not
/// always have one to give (a test purchase has none, and a restore can report
/// ownership before the receipt is back), and a bridge built before this field
/// existed simply does not append it. A record with no second field is a normal
/// owned product, not a malformed one.
fn decode_owned<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Option<(String, Option<String>)> {
    let id = unescape(fields.next()?);
    if id.is_empty() {
        return None;
    }
    let order = fields
        .next()
        .map(unescape)
        .filter(|order| !order.is_empty());
    Some((id, order))
}

fn unescape(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    value
        .replace("%09", "\t")
        .replace("%0A", "\n")
        .replace("%0D", "\r")
        .replace("%25", "%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owned_row_carries_the_order_id_when_the_store_had_one() {
        let state = decode_store_snapshot(concat!(
            "2\t0\t\n",
            "o\tcom.example.pro\tGPA.3311-9944-1234-56789\n",
            // No order id: Play has none for a test purchase, and a restore can
            // report ownership before the receipt is back. Still owned.
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
        // Nothing in Play's format forbids it, and the row is tab separated.
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
        // Billing turning off does not un-buy anything: Play still answers the
        // purchase query for an entitlement bought before, and an app that
        // dropped it here would lock a paying user out.
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
}
