#![allow(unsafe_code)]

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};

use cranpose_services::purchases::{PurchaseEvent, Purchases, StoreState, set_platform_purchases};
use jni::{
    Env, EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    sys::jint,
};

use crate::{
    android_jni::{
        clear_pending_android_jni_exception, load_cranpose_java_class, with_android_activity_env,
    },
    android_purchase_wire::{decode_purchase_event, decode_store_snapshot},
    android_services::wake_native_loop,
};

const BILLING_CLASS: &str = "dev/cranpose/android/CranposeBilling";

static SNAPSHOT: Mutex<Option<StoreState>> = Mutex::new(None);

static EVENTS: Mutex<VecDeque<PurchaseEvent>> = Mutex::new(VecDeque::new());

const MAX_PENDING_EVENTS: usize = 32;

fn snapshot() -> MutexGuard<'static, Option<StoreState>> {
    SNAPSHOT.lock().unwrap_or_else(|error| error.into_inner())
}

fn events() -> MutexGuard<'static, VecDeque<PurchaseEvent>> {
    EVENTS.lock().unwrap_or_else(|error| error.into_inner())
}

pub(crate) fn register(app: android_activity::AndroidApp) {
    let bridge = with_android_activity_env(&app, |env, activity| {
        load_cranpose_java_class(env, &activity, BILLING_CLASS).map(|_| ())
    });
    match bridge {
        Ok(()) => set_platform_purchases(Arc::new(AndroidPurchases {
            app,
            product_ids: Mutex::new(Vec::new()),
        })),
        Err(error) => log::warn!(
            "Play Billing is unavailable and nothing can be bought; \
             add cranpose/android/java-billing to the Android source set \
             and the Play Billing Gradle dependency: {error}"
        ),
    }
}

struct AndroidPurchases {
    app: android_activity::AndroidApp,
    product_ids: Mutex<Vec<String>>,
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
        *self
            .product_ids
            .lock()
            .unwrap_or_else(|error| error.into_inner()) =
            product_ids.iter().map(|id| (*id).to_owned()).collect();
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
                &[JValue::Object(&activity)],
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

    fn is_connected(&self) -> bool {
        with_android_activity_env(&self.app, |env, activity| {
            let class = load_cranpose_java_class(env, &activity, BILLING_CLASS)?;
            env.call_static_method(
                class,
                jni_str!("cranposeBillingIsConnected"),
                jni_sig!("(Landroid/app/Activity;)Z"),
                &[JValue::Object(&activity)],
            )
            .and_then(|value| value.z())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        })
        .unwrap_or(false)
    }

    fn reconnect(&self) {
        let ids = self
            .product_ids
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
        self.configure(&refs);
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
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
        Outcome::Err(_) | Outcome::Panic(_) => return,
    };
    *snapshot() = Some(decode_store_snapshot(&payload));
    cranpose_services::note_store_news();
    wake_native_loop();
}

#[doc(hidden)]
#[unsafe(no_mangle)]
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
