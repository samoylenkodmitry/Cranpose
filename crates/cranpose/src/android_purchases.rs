//! The Google Play Billing backend for [`cranpose_services::purchases`] — the
//! JNI counterpart of `dev.cranpose.android.CranposeBilling`.
//!
//! Rust → Java goes through static methods on the bridge class, loaded through
//! the activity's class loader, with the activity passed in as an argument:
//! the payment sheet is an activity result, and `launchBillingFlow` has to run
//! on the Java UI thread, which the bridge hops to itself. Nothing here waits
//! for the store — every call returns as soon as the JNI call does, which is
//! what keeps the frame thread free while a purchase is in flight.
//!
//! Java → Rust arrives on the billing library's own threads via the exported
//! `Java_dev_cranpose_android_CranposeBilling_*` symbols below. Those must not
//! touch the composition (which lives on the native-activity thread), so they
//! park a decoded snapshot and one-shot events behind mutexes and wake the
//! native loop; [`AndroidPurchases`] hands them to the frame loop the next
//! time it reads the store.
//!
//! The reviewed FFI boundary for Play Billing: the exported symbols the Java
//! bridge calls back through, and nothing else.
#![allow(unsafe_code)]

use crate::android_jni::{
    clear_pending_android_jni_exception, load_cranpose_java_class, with_android_activity_env,
};
use crate::android_purchase_wire::{decode_purchase_event, decode_store_snapshot};
use crate::android_services::wake_native_loop;
use cranpose_services::purchases::{set_platform_purchases, PurchaseEvent, Purchases, StoreState};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::jint;
use jni::{jni_sig, jni_str, Env, EnvUnowned, Outcome};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard};

/// The Java bridge, in JNI slash notation. It lives in
/// `crates/cranpose/android/java-billing`, a source directory an app adds
/// alongside the Play Billing Gradle dependency.
const BILLING_CLASS: &str = "dev/cranpose/android/CranposeBilling";

/// The most recent snapshot Java pushed. `None` until the first push, which is
/// reported as the default [`StoreState`] — no store, nothing owned — so an
/// app that reads the store before the bridge has answered is told the truth
/// rather than an optimistic guess.
static SNAPSHOT: Mutex<Option<StoreState>> = Mutex::new(None);

/// One-shot events waiting for the frame loop to drain them.
static EVENTS: Mutex<VecDeque<PurchaseEvent>> = Mutex::new(VecDeque::new());

/// Cap on undrained events. A UI that never asks for them — a headless run, a
/// screen the user never opens — must not grow memory forever.
const MAX_PENDING_EVENTS: usize = 32;

/// A poisoned mutex would mean a panic inside a callback; the state is plain
/// data, so recovering and carrying on is strictly better for the user than
/// propagating the panic through a Play Billing worker thread.
fn snapshot() -> MutexGuard<'static, Option<StoreState>> {
    SNAPSHOT.lock().unwrap_or_else(|error| error.into_inner())
}

fn events() -> MutexGuard<'static, VecDeque<PurchaseEvent>> {
    EVENTS.lock().unwrap_or_else(|error| error.into_inner())
}

/// Installs Play Billing as the platform purchase backend.
///
/// The bridge class is loaded here rather than on the first purchase so a
/// build that enabled the feature without adding the Java source directory
/// says so once, at startup, instead of failing silently at the moment the
/// user taps buy. When it cannot be loaded no backend is installed at all, and
/// [`cranpose_services::purchases`] keeps reporting
/// [`StorePhase::Unavailable`](cranpose_services::purchases::StorePhase::Unavailable).
pub(crate) fn register(app: android_activity::AndroidApp) {
    let bridge = with_android_activity_env(&app, |env, activity| {
        load_cranpose_java_class(env, &activity, BILLING_CLASS).map(|_| ())
    });
    match bridge {
        Ok(()) => set_platform_purchases(Rc::new(AndroidPurchases { app })),
        Err(error) => log::warn!(
            "Play Billing is unavailable and nothing can be bought; \
             add cranpose/android/java-billing to the Android source set \
             and the Play Billing Gradle dependency: {error}"
        ),
    }
}

/// The Play Billing backend. Every method is a single JNI call that returns
/// immediately; answers arrive later through the callbacks below.
struct AndroidPurchases {
    app: android_activity::AndroidApp,
}

impl AndroidPurchases {
    fn call(
        &self,
        what: &'static str,
        run: impl for<'local> FnOnce(
            &mut Env<'local>,
            &JObject<'local>,
            JClass<'local>,
        ) -> Result<(), String>,
    ) {
        let result = with_android_activity_env(&self.app, |env, activity| {
            let class = load_cranpose_java_class(env, &activity, BILLING_CLASS)?;
            run(env, &activity, class)
        });
        if let Err(error) = result {
            log::warn!("Android billing {what} failed: {error}");
        }
    }
}

impl Purchases for AndroidPurchases {
    fn configure(&self, product_ids: &[&str]) {
        // Newline-separated: Play product ids are `[a-z0-9._]`, so a newline
        // cannot occur inside one and no escaping is needed on the way out.
        let joined = product_ids.join("\n");
        self.call("configure", move |env, activity, class| {
            let ids = env.new_string(&joined).map_err(|error| error.to_string())?;
            let ids_object: &JObject = ids.as_ref();
            env.call_static_method(
                class,
                jni_str!("cranposeBillingConfigure"),
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)V"),
                &[JValue::Object(activity), JValue::Object(ids_object)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }

    fn state(&self) -> StoreState {
        // Called every frame: one uncontended lock and a clone of a short
        // product list, with no JNI call and no allocation on the store's
        // behalf.
        snapshot().clone().unwrap_or_default()
    }

    fn purchase(&self, product_id: &str) {
        let product_id = product_id.to_string();
        self.call("purchase", move |env, activity, class| {
            let id = env
                .new_string(&product_id)
                .map_err(|error| error.to_string())?;
            let id_object: &JObject = id.as_ref();
            env.call_static_method(
                class,
                jni_str!("cranposeBillingPurchase"),
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)V"),
                &[JValue::Object(activity), JValue::Object(id_object)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }

    fn restore(&self) {
        self.call("restore", |env, activity, class| {
            env.call_static_method(
                class,
                jni_str!("cranposeBillingRestore"),
                jni_sig!("(Landroid/app/Activity;)V"),
                &[JValue::Object(activity)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }

    fn take_event(&self) -> Option<PurchaseEvent> {
        events().pop_front()
    }
}

// --- Java → Rust callbacks (Play Billing worker threads) ---------------------

/// The billing client learned something: connection phase, prices, or what the
/// account owns. The payload is the whole snapshot, so it replaces the
/// previous one wholesale.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeBilling_nativeBillingSnapshot<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    payload: JString<'local>,
) {
    let payload = match env
        .with_env(|env| -> jni::errors::Result<String> { payload.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(payload) => payload,
        // A payload that cannot be read is dropped rather than decoded as an
        // empty one: losing an update is recoverable, blanking a price list
        // the user is reading is not.
        Outcome::Err(_) | Outcome::Panic(_) => return,
    };
    *snapshot() = Some(decode_store_snapshot(&payload));
    // Tell the app, rather than leaving it to ask. Waking the loop alone means
    // the store is only ever read by an app that re-reads it every frame --
    // and an app that has gone idle, which is the right thing to be on a
    // screen showing a price, has no frame to re-read it from.
    cranpose_services::note_store_news();
    wake_native_loop();
}

/// Something happened that a snapshot cannot express — the user cancelled, the
/// payment is waiting on someone else, a restore finished.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeBilling_nativeBillingEvent<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    code: jint,
    message: JString<'local>,
    count: jint,
) {
    let message = match env
        .with_env(|env| -> jni::errors::Result<String> { message.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(message) => message,
        Outcome::Err(_) | Outcome::Panic(_) => String::new(),
    };
    let Some(event) = decode_purchase_event(code, message, count) else {
        return;
    };
    let mut events = events();
    if events.len() >= MAX_PENDING_EVENTS {
        events.pop_front();
    }
    events.push_back(event);
    drop(events);
    cranpose_services::note_store_news();
    wake_native_loop();
}
