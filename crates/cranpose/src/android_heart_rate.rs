#![expect(unsafe_code)]

//! The heart-rate sensor on Android, through `CranposeHeartRate`, which an
//! application carries only when its build script declares
//! `Use::heart_rate`. The permission answer comes back through
//! `CranposeActivity#onRequestPermissionsResult`, which hands Rust every
//! answer to a request the activity did not make itself.

use std::sync::Arc;

use cranpose_services::{
    HeartRate, HeartRateError, HeartRateMonitor, HeartRatePermission, HeartRateStatus,
    publish_heart_rate, publish_heart_rate_permission, set_platform_heart_rate,
};
use jni::{
    Env, EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    sys::{jfloat, jint},
};

use crate::android_jni::{
    clear_pending_android_jni_exception, load_cranpose_java_class, with_android_activity_env,
};

const HEART_RATE_CLASS: &str = "dev/cranpose/android/CranposeHeartRate";

/// The permissions the sensor is read under, the newer one first.
const HEART_RATE_PERMISSIONS: [&str; 2] = [
    "android.permission.health.READ_HEART_RATE",
    "android.permission.BODY_SENSORS",
];

pub(crate) fn register(app: android_activity::AndroidApp) {
    set_platform_heart_rate(Arc::new(AndroidHeartRate { app }));
}

struct AndroidHeartRate {
    app: android_activity::AndroidApp,
}

impl AndroidHeartRate {
    fn call<T>(
        &self,
        run: impl for<'local> FnOnce(
            &mut Env<'local>,
            &JObject<'local>,
            JClass<'local>,
        ) -> jni::errors::Result<T>,
    ) -> Result<T, String> {
        with_android_activity_env(&self.app, |env, activity| {
            let class = load_cranpose_java_class(env, &activity, HEART_RATE_CLASS)?;
            run(env, &activity, class).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        })
    }

    fn call_void(&self, name: &'static jni::strings::JNIStr) -> Result<(), String> {
        self.call(|env, activity, class| {
            env.call_static_method(
                class,
                name,
                jni_sig!("(Landroid/app/Activity;)V"),
                &[JValue::Object(activity)],
            )
            .map(|_| ())
        })
    }
}

impl HeartRateMonitor for AndroidHeartRate {
    fn available(&self) -> bool {
        self.call(|env, activity, class| {
            env.call_static_method(
                class,
                jni_str!("cranposeHeartRateAvailable"),
                jni_sig!("(Landroid/content/Context;)Z"),
                &[JValue::Object(activity)],
            )
            .and_then(jni::JValueOwned::z)
        })
        .unwrap_or(false)
    }

    fn permission(&self) -> HeartRatePermission {
        let answer = self.call(|env, activity, class| {
            env.call_static_method(
                class,
                jni_str!("cranposeHeartRatePermission"),
                jni_sig!("(Landroid/app/Activity;)I"),
                &[JValue::Object(activity)],
            )
            .and_then(jni::JValueOwned::i)
        });
        match answer {
            Ok(1) => HeartRatePermission::Granted,
            Ok(2) => HeartRatePermission::Denied,
            Ok(_) => HeartRatePermission::NotAsked,
            Err(_) => HeartRatePermission::Denied,
        }
    }

    fn request_permission(&self) {
        let _ = self.call_void(jni_str!("cranposeHeartRateRequestPermission"));
    }

    fn start(&self) -> Result<(), HeartRateError> {
        self.call_void(jni_str!("cranposeHeartRateStart"))
            .map_err(HeartRateError::Failed)
    }

    fn stop(&self) {
        let _ = self.call_void(jni_str!("cranposeHeartRateStop"));
    }
}

/// A reading or a change of state from the sensor: `status` is
/// `CranposeHeartRate`'s ACQUIRING, LIVE or OFF_BODY.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeHeartRate_nativeOnHeartRate(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    bpm: jfloat,
    status: jint,
) {
    publish_heart_rate(reading(bpm, status));
}

fn reading(bpm: f32, status: i32) -> HeartRate {
    match status {
        1 if bpm > 0.0 => HeartRate {
            status: HeartRateStatus::Live,
            bpm: Some(bpm),
        },
        2 => HeartRate {
            status: HeartRateStatus::OffBody,
            bpm: None,
        },
        _ => HeartRate {
            status: HeartRateStatus::Acquiring,
            bpm: None,
        },
    }
}

/// Every answer to a permission request the activity did not make itself,
/// one `permission<TAB>granted` line each, routed to the service that asked.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnPermissionsResult<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    results: JString<'local>,
) {
    let decoded = env.with_env(|env| results.try_to_string(env));
    let Outcome::Ok(results) = decoded.into_outcome() else {
        return;
    };
    if let Some(granted) = heart_rate_answer(&results) {
        publish_heart_rate_permission(granted);
    }
}

/// The heart-rate permission's answer among `results`, if it is there.
fn heart_rate_answer(results: &str) -> Option<bool> {
    results.lines().find_map(|line| {
        let (name, granted) = line.split_once('\t')?;
        HEART_RATE_PERMISSIONS
            .contains(&name)
            .then_some(granted == "1")
    })
}

#[cfg(test)]
#[path = "tests/android_heart_rate_tests.rs"]
mod tests;
