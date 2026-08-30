#![allow(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::{CStr, CString, c_char, c_void},
    sync::{Arc, Mutex},
};

use cranpose_services::purchases::{
    Product, PurchaseEvent, Purchases, StorePhase, StoreState, set_platform_purchases,
};

const KIND_BEGIN: i32 = 0;
const KIND_PRODUCT: i32 = 1;
const KIND_OWNED: i32 = 2;
const KIND_PHASE: i32 = 3;
const KIND_EVENT: i32 = 4;
const KIND_BUSY: i32 = 5;

const PHASE_UNAVAILABLE: i32 = 0;
const PHASE_CONNECTING: i32 = 1;
const PHASE_READY: i32 = 2;
const PHASE_BLOCKED: i32 = 3;

const EVENT_PURCHASED: i32 = 0;
const EVENT_CANCELLED: i32 = 1;
const EVENT_PENDING: i32 = 2;
const EVENT_FAILED: i32 = 3;
const EVENT_RESTORED: i32 = 4;

type StoreCallback = unsafe extern "C" fn(
    *mut c_void,
    i32,
    i32,
    i32,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
);

unsafe extern "C" {
    fn cranpose_storekit_start(product_ids: *const c_char, ctx: *mut c_void, cb: StoreCallback);
    fn cranpose_storekit_is_connected() -> bool;
    fn cranpose_storekit_purchase(product_id: *const c_char);
    fn cranpose_storekit_restore();
}

struct Shared {
    staging: bool,
    staged_products: Vec<Product>,
    staged_owned: BTreeSet<String>,
    staged_orders: BTreeMap<String, String>,
    live: StoreState,
    events: VecDeque<PurchaseEvent>,
}

impl Shared {
    const fn new() -> Self {
        Self {
            staging: false,
            staged_products: Vec::new(),
            staged_owned: BTreeSet::new(),
            staged_orders: BTreeMap::new(),
            live: StoreState {
                phase: StorePhase::Unavailable,
                products: Vec::new(),
                owned: BTreeSet::new(),
                orders: BTreeMap::new(),
                error: None,
                busy: false,
            },
            events: VecDeque::new(),
        }
    }
}

static SHARED: Mutex<Shared> = Mutex::new(Shared::new());

fn shared() -> std::sync::MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe fn take(ptr: *const c_char) -> Option<String> {
    unsafe {
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

unsafe extern "C" fn on_message(
    _ctx: *mut c_void,
    kind: i32,
    arg0: i32,
    arg1: i32,
    a: *const c_char,
    b: *const c_char,
    c: *const c_char,
    d: *const c_char,
) {
    unsafe {
        let mut state = shared();
        match kind {
            KIND_BEGIN => {
                state.staging = true;
                state.staged_products.clear();
                state.staged_owned.clear();
                state.staged_orders.clear();
            }
            KIND_PRODUCT => {
                let (Some(id), Some(display_price)) = (take(a), take(b)) else {
                    return;
                };
                state.staged_products.push(Product {
                    id,
                    display_price,
                    title: take(c).unwrap_or_default(),
                    description: take(d).unwrap_or_default(),
                });
            }
            KIND_OWNED => {
                if let Some(id) = take(a) {
                    if let Some(order) = take(b) {
                        state.staged_orders.insert(id.clone(), order);
                    }
                    state.staged_owned.insert(id);
                }
            }
            KIND_PHASE => {
                if state.staging {
                    state.live.products = std::mem::take(&mut state.staged_products);
                    state.live.owned = std::mem::take(&mut state.staged_owned);
                    state.live.orders = std::mem::take(&mut state.staged_orders);
                    state.staging = false;
                }
                state.live.phase = match arg0 {
                    PHASE_READY => StorePhase::Ready,
                    PHASE_CONNECTING => StorePhase::Connecting,
                    PHASE_BLOCKED => StorePhase::Blocked,
                    PHASE_UNAVAILABLE => StorePhase::Unavailable,
                    _ => StorePhase::Unavailable,
                };
                state.live.error = take(a);
            }
            KIND_BUSY => state.live.busy = arg0 != 0,
            KIND_EVENT => {
                let event = match arg0 {
                    EVENT_PURCHASED => PurchaseEvent::Purchased(take(a).unwrap_or_default()),
                    EVENT_CANCELLED => PurchaseEvent::Cancelled,
                    EVENT_PENDING => PurchaseEvent::Pending,
                    EVENT_FAILED => PurchaseEvent::Failed(
                        take(a)
                            .unwrap_or_else(|| "The purchase could not be completed".to_string()),
                    ),
                    EVENT_RESTORED => PurchaseEvent::Restored {
                        restored: arg1.max(0) as usize,
                    },
                    _ => return,
                };
                if state.events.len() >= 32 {
                    state.events.pop_front();
                }
                state.events.push_back(event);
                drop(state);
                cranpose_services::note_store_news();
                return;
            }
            _ => {}
        }
        drop(state);
        cranpose_services::note_store_news();
    }
}

/// The App Store backend. Install it with [`register`].
pub struct StoreKitPurchases;

static STOREKIT_PRODUCT_IDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

impl Purchases for StoreKitPurchases {
    fn configure(&self, product_ids: &[&str]) {
        *STOREKIT_PRODUCT_IDS
            .lock()
            .unwrap_or_else(|error| error.into_inner()) =
            product_ids.iter().map(|id| (*id).to_owned()).collect();
        let joined = product_ids.join("\n");
        let Ok(joined) = CString::new(joined) else {
            return;
        };
        unsafe {
            cranpose_storekit_start(joined.as_ptr(), std::ptr::null_mut(), on_message);
        }
    }

    fn state(&self) -> StoreState {
        shared().live.clone()
    }

    fn purchase(&self, product_id: &str) {
        let Ok(id) = CString::new(product_id) else {
            return;
        };
        unsafe { cranpose_storekit_purchase(id.as_ptr()) }
    }

    fn restore(&self) {
        unsafe { cranpose_storekit_restore() }
    }

    fn take_event(&self) -> Option<PurchaseEvent> {
        shared().events.pop_front()
    }

    fn is_connected(&self) -> bool {
        unsafe { cranpose_storekit_is_connected() }
    }

    fn reconnect(&self) {
        let ids = STOREKIT_PRODUCT_IDS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
        self.configure(&refs);
    }
}

/// Installs StoreKit as the platform purchase backend.
pub fn register() {
    set_platform_purchases(Arc::new(StoreKitPurchases));
}

#[cfg(test)]
mod tests {
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
}
