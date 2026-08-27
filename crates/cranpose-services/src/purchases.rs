//! In-app purchases: products, prices and owned entitlements.
//!
//! The shape is the one every mobile store agrees on — ask for a set of
//! product ids, get back localized prices, start a purchase, and be told what
//! the account owns — with the store-specific parts (StoreKit, Play Billing)
//! living in platform backends installed via [`set_platform_purchases`].
//!
//! **The default backend reports [`StorePhase::Unavailable`] and owns
//! nothing.** It deliberately does *not* grant entitlements: a desktop build
//! with no store must not silently unlock paid features because a backend
//! failed to register. An app that ships free on storeless platforms decides
//! that itself, e.g.
//!
//! ```no_run
//! # use cranpose_services::purchases::store_state;
//! let state = store_state();
//! // No store that will sell here — this app is free in that case.
//! let unlocked = state.phase.cannot_sell() || state.owns("com.example.pro");
//! ```
//!
//! The two phases that cannot sell say different things to a *user*:
//! [`StorePhase::Unavailable`] is "not reached, try again", and
//! [`StorePhase::Blocked`] is "this store will not sell to you here". Offering
//! a retry for the second one only fails the same way again.
//!
//! # Reading state
//!
//! [`store_state`] is a cheap snapshot, safe to call every frame: backends
//! keep the state and hand out a clone. State changes arrive asynchronously
//! (the store answers over the network, another device restores a purchase,
//! a parent approves an Ask-to-Buy request), so read it from the frame loop
//! rather than expecting a reply to [`purchase`].
//!
//! [`take_event`] drains one-shot events — the things a snapshot cannot
//! express, like "the user cancelled" — for showing a message once.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::registry::{RecoveryGate, ServiceRegistry};

/// A product as the store describes it, in the user's locale and currency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Product {
    /// Store product identifier, as configured in App Store Connect or the
    /// Play Console.
    pub id: String,
    /// Price formatted by the store for the user's storefront — "$34.99",
    /// "34,99 €", "¥5,000". **Always display this string**; never format a
    /// price yourself, and never hard-code one. Stores localize currency,
    /// separators and placement, and they apply regional price tiers.
    pub display_price: String,
    /// Display name configured in the store.
    pub title: String,
    /// Description configured in the store.
    pub description: String,
}

/// How far along the store connection is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StorePhase {
    /// No store on this platform, or no backend installed, or one that has
    /// not reached the store yet. Nothing is owned and nothing can be bought
    /// *right now* — a backend that is retrying reports this, so an app may
    /// reasonably say "not reached" and offer to try again.
    #[default]
    Unavailable,
    /// The store answered, and it will not sell to this app here: in-app
    /// billing turned off on the device, an account that cannot pay, a
    /// country the app is not distributed in.
    ///
    /// The difference from [`Unavailable`](Self::Unavailable) is whether
    /// waiting helps. It does not here — no backend retries a store that has
    /// said no — so an app should stop offering the purchase and say why,
    /// rather than inviting a retry that can only fail the same way.
    /// Already-known ownership remains authoritative even though the store
    /// cannot be queried for new purchases.
    Blocked,
    /// A backend is installed and still talking to the store. Prices are not
    /// known yet; owned entitlements may not be known yet either.
    Connecting,
    /// Product and entitlement information has been received at least once.
    Ready,
}

impl StorePhase {
    /// Whether nothing can be bought in this phase.
    ///
    /// Saves every paywall from spelling out the same two-variant match, and
    /// keeps an app that only cares "can I sell?" from having to be updated
    /// when a phase is added.
    pub fn cannot_sell(self) -> bool {
        matches!(self, Self::Unavailable | Self::Blocked)
    }

    /// Whether the store might still answer differently later.
    ///
    /// True while a backend is connecting or has yet to reach the store,
    /// false once the store has said no or has already answered.
    pub fn may_yet_change(self) -> bool {
        matches!(self, Self::Unavailable | Self::Connecting)
    }
}

/// Snapshot of everything known about the store right now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreState {
    /// How far along the connection is.
    pub phase: StorePhase,
    /// Products the backend was configured with and the store answered for.
    /// A configured product missing here is one the store does not know —
    /// usually a typo in the id, or a product not yet approved.
    pub products: Vec<Product>,
    /// Product ids the account currently owns. For non-consumables and
    /// subscriptions this is the entitlement; consumables never appear.
    pub owned: BTreeSet<String>,
    /// The store's identifier for the purchase that granted each owned
    /// product — Play's order id, StoreKit's transaction id.
    ///
    /// Separate from [`owned`](Self::owned) rather than replacing it, because
    /// a backend can know that a product is owned without knowing what paid
    /// for it: Play's `queryPurchases` omits the order id for a test purchase,
    /// and a restore on a reinstalled app can report ownership before the
    /// receipt is back. Ownership is the entitlement; this is only the paper
    /// trail. Never gate access on it.
    ///
    /// An app that keeps a local record of the purchase wants it: with only
    /// the product id there is nothing to quote to the store, or to the user,
    /// if the entitlement is ever in dispute.
    pub orders: BTreeMap<String, String>,
    /// Last error reported by the store, for diagnostics. A store being
    /// briefly unreachable is normal and not worth showing to the user.
    pub error: Option<String>,
    /// True while a purchase or restore the user asked for is still running,
    /// so the UI can disable the buy button and show a spinner.
    pub busy: bool,
}

impl StoreState {
    /// Whether `product_id` is currently owned.
    pub fn owns(&self, product_id: &str) -> bool {
        self.owned.contains(product_id)
    }

    /// The store's identifier for the purchase that granted `product_id`, if
    /// the backend reported one. See [`orders`](Self::orders): absent is
    /// normal and does not mean unowned.
    pub fn order_id(&self, product_id: &str) -> Option<&str> {
        self.orders.get(product_id).map(String::as_str)
    }

    /// The product with `product_id`, if the store answered for it.
    pub fn product(&self, product_id: &str) -> Option<&Product> {
        self.products.iter().find(|p| p.id == product_id)
    }

    /// The localized price of `product_id`, if known.
    pub fn display_price(&self, product_id: &str) -> Option<&str> {
        self.product(product_id).map(|p| p.display_price.as_str())
    }
}

/// A one-shot thing that happened, which a snapshot cannot express.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PurchaseEvent {
    /// The purchase completed and the entitlement is in [`StoreState::owned`].
    Purchased(String),
    /// The user dismissed the payment sheet. Not an error; say nothing.
    Cancelled,
    /// The purchase needs someone else to finish it — Ask to Buy, or a
    /// bank-side confirmation. It may complete minutes or days later, so tell
    /// the user it is pending rather than that it failed.
    Pending,
    /// The purchase failed. The string is for the user.
    Failed(String),
    /// A restore finished. `restored` is how many entitlements it found —
    /// zero means "nothing to restore on this account", which is worth
    /// saying, because the user asked.
    Restored {
        /// Number of owned entitlements the restore turned up.
        restored: usize,
    },
}

/// A store backend.
///
/// Implementations are installed with [`set_platform_purchases`] and must be
/// non-blocking: every method returns immediately and reports back by
/// updating the snapshot returned from [`Purchases::state`].
pub trait Purchases: Send + Sync {
    /// Declare the product ids this app sells and start talking to the store.
    /// Called again on relaunch; backends should treat it as idempotent.
    fn configure(&self, product_ids: &[&str]);

    /// The current snapshot. Called every frame — keep it cheap.
    fn state(&self) -> StoreState;

    /// Begin a purchase. Presents the store's own payment sheet.
    fn purchase(&self, product_id: &str);

    /// Re-query what the account owns. Stores restore silently at launch, so
    /// this is for the explicit "Restore purchases" button that Apple
    /// requires a paid app to provide.
    fn restore(&self);

    /// Take the next pending one-shot event, if any.
    fn take_event(&self) -> Option<PurchaseEvent>;

    fn is_connected(&self) -> bool;

    fn reconnect(&self);
}

/// Shared handle to the active [`Purchases`] backend.
pub type PurchasesRef = Arc<dyn Purchases>;

/// The no-store backend: nothing is for sale and nothing is owned.
struct NoPurchases;

impl Purchases for NoPurchases {
    fn configure(&self, _product_ids: &[&str]) {}

    fn state(&self) -> StoreState {
        StoreState::default()
    }

    fn purchase(&self, _product_id: &str) {}

    fn restore(&self) {}

    fn take_event(&self) -> Option<PurchaseEvent> {
        None
    }

    fn is_connected(&self) -> bool {
        false
    }

    fn reconnect(&self) {}
}

static PLATFORM_PURCHASES: ServiceRegistry<dyn Purchases> = ServiceRegistry::new();
static NO_PURCHASES: OnceLock<PurchasesRef> = OnceLock::new();
static DEFAULT_PURCHASES: OnceLock<PurchasesRef> = OnceLock::new();
static PURCHASE_RECOVERY: RecoveryGate = RecoveryGate::new();

struct PlatformPurchases;

fn registered_purchases() -> PurchasesRef {
    PLATFORM_PURCHASES
        .get_or_warn("purchases")
        .unwrap_or_else(|| NO_PURCHASES.get_or_init(|| Arc::new(NoPurchases)).clone())
}

fn active_purchases() -> PurchasesRef {
    let purchases = registered_purchases();
    if purchases.is_connected() {
        PURCHASE_RECOVERY.succeeded();
    } else if PURCHASE_RECOVERY.try_start() {
        purchases.reconnect();
    }
    purchases
}

impl Purchases for PlatformPurchases {
    fn configure(&self, product_ids: &[&str]) {
        active_purchases().configure(product_ids);
    }

    fn state(&self) -> StoreState {
        active_purchases().state()
    }

    fn purchase(&self, product_id: &str) {
        active_purchases().purchase(product_id);
    }

    fn restore(&self) {
        active_purchases().restore();
    }

    fn take_event(&self) -> Option<PurchaseEvent> {
        active_purchases().take_event()
    }

    fn is_connected(&self) -> bool {
        registered_purchases().is_connected()
    }

    fn reconnect(&self) {
        registered_purchases().reconnect();
    }
}

/// Installs a platform purchase backend, replacing any previous one.
#[cfg(not(target_arch = "wasm32"))]
type StoreListener = Arc<dyn Fn() + Send + Sync>;
#[cfg(target_arch = "wasm32")]
type StoreListener = std::rc::Rc<dyn Fn()>;

#[cfg(not(target_arch = "wasm32"))]
fn store_listeners() -> &'static Mutex<Vec<(u64, StoreListener)>> {
    static LISTENERS: OnceLock<Mutex<Vec<(u64, StoreListener)>>> = OnceLock::new();
    LISTENERS.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static STORE_LISTENERS: std::cell::RefCell<Vec<(u64, StoreListener)>> = const { std::cell::RefCell::new(Vec::new()) };
}

static NEXT_STORE_LISTENER_ID: AtomicU64 = AtomicU64::new(1);

/// A store observer installed by [`observe_store_news`].
pub struct StoreObserver {
    id: u64,
}

impl Drop for StoreObserver {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut listeners) = store_listeners().lock() {
            listeners.retain(|(id, _)| *id != self.id);
        }
        #[cfg(target_arch = "wasm32")]
        STORE_LISTENERS.with(|listeners| listeners.borrow_mut().retain(|(id, _)| *id != self.id));
    }
}

/// Registers a callback run whenever the store has news, so an app can be told
/// rather than having to ask.
///
/// [`take_event`] and [`store_state`] are polling APIs, which assume the app is
/// already running a frame loop to poll from. An app that has gone idle has no
/// such loop, so a purchase that finishes while nothing moves on screen sits in
/// the queue until something unrelated wakes the app. The listener closes that
/// gap: it is the nudge, the queue is still the source of truth.
///
/// Native callbacks can arrive from a platform thread and therefore must be
/// `Send + Sync`. Browser callbacks remain on their browser thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn observe_store_news(listener: impl Fn() + Send + Sync + 'static) -> StoreObserver {
    let id = NEXT_STORE_LISTENER_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut listeners) = store_listeners().lock() {
        listeners.push((id, Arc::new(listener)));
    }
    StoreObserver { id }
}

/// Registers a browser-thread callback run whenever the store has news.
#[cfg(target_arch = "wasm32")]
pub fn observe_store_news(listener: impl Fn() + 'static) -> StoreObserver {
    let id = NEXT_STORE_LISTENER_ID.fetch_add(1, Ordering::Relaxed);
    STORE_LISTENERS.with(|listeners| {
        listeners
            .borrow_mut()
            .push((id, std::rc::Rc::new(listener)))
    });
    StoreObserver { id }
}

/// Tells the app that the store has news. Called by a purchase backend.
pub fn note_store_news() {
    #[cfg(not(target_arch = "wasm32"))]
    let listeners = store_listeners()
        .lock()
        .map(|listeners| {
            listeners
                .iter()
                .map(|(_, listener)| Arc::clone(listener))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    #[cfg(target_arch = "wasm32")]
    let listeners = STORE_LISTENERS.with(|listeners| {
        listeners
            .borrow()
            .iter()
            .map(|(_, listener)| std::rc::Rc::clone(listener))
            .collect::<Vec<_>>()
    });
    for listener in listeners {
        listener();
    }
}

pub fn set_platform_purchases(purchases: PurchasesRef) {
    PLATFORM_PURCHASES.set(purchases);
    PURCHASE_RECOVERY.succeeded();
}

/// Removes any registered purchase backend (tests and teardown).
pub fn clear_platform_purchases() {
    PLATFORM_PURCHASES.clear();
}

/// The active backend: the platform one if installed, else the no-store
/// backend.
pub fn purchases() -> PurchasesRef {
    DEFAULT_PURCHASES
        .get_or_init(|| Arc::new(PlatformPurchases))
        .clone()
}

/// Whether a real store backend is installed on this platform.
pub fn store_available() -> bool {
    PLATFORM_PURCHASES.get().is_some()
}

/// Convenience: declare the products this app sells and connect to the store.
pub fn configure(product_ids: &[&str]) {
    purchases().configure(product_ids);
}

/// Convenience: the current store snapshot.
pub fn store_state() -> StoreState {
    purchases().state()
}

/// Convenience: begin a purchase.
pub fn purchase(product_id: &str) {
    purchases().purchase(product_id);
}

/// Convenience: re-query owned entitlements.
pub fn restore() {
    purchases().restore();
}

/// Convenience: take the next one-shot purchase event.
pub fn take_event() -> Option<PurchaseEvent> {
    purchases().take_event()
}

/// The store's current state, observed for as long as this call stays in the
/// composition.
///
/// The composition recomposes when the backend publishes news; nothing polls
/// and no frame loop is required for a purchase that completes while the screen
/// is idle.
#[allow(non_snake_case)]
#[track_caller]
pub fn rememberStoreState() -> cranpose_core::State<StoreState> {
    let updates = cranpose_core::rememberEventStream((), |sender| {
        observe_store_news(move || sender.send(store_state()))
    });
    cranpose_core::collectAsState(updates, (), store_state())
}

/// One-shot purchase outcomes as a composition-scoped stream.
///
/// Each event is delivered exactly once. Collect it with
/// [`cranpose_core::CollectEvents`].
#[allow(non_snake_case)]
#[track_caller]
pub fn rememberPurchaseEvents() -> cranpose_core::EventStream<PurchaseEvent> {
    cranpose_core::rememberEventStream((), |sender| {
        observe_store_news(move || {
            // The backend queues events and nudges; draining here is the
            // framework's job, and the application only ever sees the stream.
            while let Some(event) = take_event() {
                sender.send(event);
            }
        })
    })
}

#[cfg(test)]
mod tests {
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
        // Calling through with no backend must not panic.
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
        // The default has to stay the phase that invites a retry: a backend
        // that has not answered yet must not read as one that refused.
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
        // Absent is normal: a backend can know a product is owned without
        // knowing what paid for it, so this must never read as "not owned".
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
}
