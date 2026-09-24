use std::collections::{BTreeMap, BTreeSet};

use cranpose_services::purchases::{Product, PurchaseEvent, StorePhase, StoreState};

use crate::android_wire_escape::unescape_wire_field as unescape;

pub(crate) const EVENT_PURCHASED: i32 = 0;
pub(crate) const EVENT_CANCELLED: i32 = 1;
pub(crate) const EVENT_PENDING: i32 = 2;
pub(crate) const EVENT_FAILED: i32 = 3;
pub(crate) const EVENT_RESTORED: i32 = 4;

pub(crate) fn decode_store_snapshot(payload: &str) -> StoreState {
    let mut lines = payload.split('\n');
    let mut header = lines.next().unwrap_or_default().split('\t');
    let phase = match header.next() {
        Some("1") => StorePhase::Connecting,
        Some("2") => StorePhase::Ready,
        Some("3") => StorePhase::Blocked,
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
        title: fields.next().map(unescape).unwrap_or_default(),
        description: fields.next().map(unescape).unwrap_or_default(),
    })
}

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

#[cfg(test)]
#[path = "tests/android_purchase_wire_tests.rs"]
mod tests;
