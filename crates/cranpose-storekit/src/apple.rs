//! The Apple half: FFI to `swift/storekit.swift` plus the
//! [`cranpose_services::purchases::Purchases`] implementation.
//!
//! The reviewed FFI boundary for this crate — the crate root denies unsafe
//! code and this is the only module that opts back in.
#![allow(unsafe_code)]

use cranpose_services::purchases::{
    set_platform_purchases, Product, PurchaseEvent, Purchases, StorePhase, StoreState,
};
use std::collections::{BTreeSet, VecDeque};
use std::ffi::{c_char, c_void, CStr, CString};
use std::rc::Rc;
use std::sync::Mutex;

// ---------------------------------------------------------------- ABI codes
//
// These mirror the `Kind`, `PhaseCode` and `EventCode` enums in
// `swift/storekit.swift`. Both sides are in this crate and change together.

const KIND_BEGIN: i32 = 0;
const KIND_PRODUCT: i32 = 1;
const KIND_OWNED: i32 = 2;
const KIND_PHASE: i32 = 3;
const KIND_EVENT: i32 = 4;
const KIND_BUSY: i32 = 5;

const PHASE_UNAVAILABLE: i32 = 0;
const PHASE_CONNECTING: i32 = 1;
const PHASE_READY: i32 = 2;

const EVENT_PURCHASED: i32 = 0;
const EVENT_CANCELLED: i32 = 1;
const EVENT_PENDING: i32 = 2;
const EVENT_FAILED: i32 = 3;
const EVENT_RESTORED: i32 = 4;

/// `(ctx, kind, arg0, arg1, a, b, c, d)` — see `swift/storekit.swift`.
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

extern "C" {
    fn cranpose_storekit_start(product_ids: *const c_char, ctx: *mut c_void, cb: StoreCallback);
    fn cranpose_storekit_purchase(product_id: *const c_char);
    fn cranpose_storekit_restore();
}

// -------------------------------------------------------------------- state

/// Everything the Swift side has told us.
///
/// A snapshot arrives as `begin`, a run of `product`/`owned` rows, then
/// `phase`, which swaps the staged rows into `live` in one step — so a reader
/// never observes a half-built product list.
struct Shared {
    staging: bool,
    staged_products: Vec<Product>,
    staged_owned: BTreeSet<String>,
    live: StoreState,
    events: VecDeque<PurchaseEvent>,
}

impl Shared {
    const fn new() -> Self {
        Self {
            staging: false,
            staged_products: Vec::new(),
            staged_owned: BTreeSet::new(),
            live: StoreState {
                phase: StorePhase::Unavailable,
                products: Vec::new(),
                owned: BTreeSet::new(),
                error: None,
                busy: false,
            },
            events: VecDeque::new(),
        }
    }
}

static SHARED: Mutex<Shared> = Mutex::new(Shared::new());

/// A poisoned mutex would mean a panic inside the callback; the state is
/// plain data, so recovering and carrying on is strictly better for the user
/// than propagating the panic through a StoreKit executor.
fn shared() -> std::sync::MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

/// Borrow a C string as an owned `String`. `None` for null.
///
/// # Safety
/// `ptr` is either null or a NUL-terminated string valid for this call, which
/// is what the Swift side guarantees (its buffers live for the callback's
/// duration only, so the copy here is required, not an optimization).
unsafe fn take(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
}

/// The one callback the Swift side pushes everything through.
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
    let mut state = shared();
    match kind {
        KIND_BEGIN => {
            state.staging = true;
            state.staged_products.clear();
            state.staged_owned.clear();
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
                state.staged_owned.insert(id);
            }
        }
        KIND_PHASE => {
            // Commit, but only if rows were staged: a bare phase update (the
            // "connecting" ping at start-up) must not blank a price list the
            // user is already looking at.
            if state.staging {
                state.live.products = std::mem::take(&mut state.staged_products);
                state.live.owned = std::mem::take(&mut state.staged_owned);
                state.staging = false;
            }
            state.live.phase = match arg0 {
                PHASE_READY => StorePhase::Ready,
                PHASE_CONNECTING => StorePhase::Connecting,
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
                    take(a).unwrap_or_else(|| "The purchase could not be completed".to_string()),
                ),
                EVENT_RESTORED => PurchaseEvent::Restored {
                    restored: arg1.max(0) as usize,
                },
                _ => return,
            };
            // Bound the queue: a UI that never drains events (a headless run,
            // a screen the user never opens) must not grow memory forever.
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

// ------------------------------------------------------------------ backend

/// The App Store backend. Install it with [`register`].
pub struct StoreKitPurchases;

impl Purchases for StoreKitPurchases {
    fn configure(&self, product_ids: &[&str]) {
        // Newline-separated: product ids are `[A-Za-z0-9._-]` on both stores,
        // so a newline cannot occur inside one and no escaping is needed.
        let joined = product_ids.join("\n");
        let Ok(joined) = CString::new(joined) else {
            return;
        };
        // SAFETY: `joined` outlives the call; the Swift side copies what it
        // needs before returning. `on_message` has the matching ABI and takes
        // no context (its state is the `SHARED` static).
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
        // SAFETY: `id` outlives the call and is NUL-terminated.
        unsafe { cranpose_storekit_purchase(id.as_ptr()) }
    }

    fn restore(&self) {
        // SAFETY: no arguments; the Swift side is a no-op before `configure`.
        unsafe { cranpose_storekit_restore() }
    }

    fn take_event(&self) -> Option<PurchaseEvent> {
        shared().events.pop_front()
    }
}

/// Installs StoreKit as the platform purchase backend.
pub fn register() {
    set_platform_purchases(Rc::new(StoreKitPurchases));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the callback exactly as Swift does and assert the snapshot is
    /// committed atomically, with prices surviving a bare phase update.
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
            // Still staging: nothing visible yet.
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

        // A "connecting" ping with no rows must not wipe the committed list.
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
        // Start from a known state; the snapshot test shares the static.
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

        // An undrained queue is capped, not unbounded.
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
