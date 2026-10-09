#![expect(unsafe_code)]

//! The heart-rate sensor on Android, through `CranposeHeartRate`, which an
//! application carries only when its build script declares
//! `Use::heart_rate`. The permission answer comes back through
//! `android_permissions`.

use std::sync::Arc;

use cranpose_services::{
    HeartRate, HeartRateError, HeartRateMonitor, HeartRatePermission, HeartRateStatus,
    publish_heart_rate, set_platform_heart_rate,
};
use jni::{
    EnvUnowned, jni_sig, jni_str,
    objects::{JClass, JValue},
    sys::{jfloat, jint},
};

use crate::android_jni::{call_cranpose_activity_method, call_cranpose_class};

const HEART_RATE_CLASS: &str = "dev/cranpose/android/CranposeHeartRate";

/// The permissions the sensor is read under, the newer one first.
pub(crate) const HEART_RATE_PERMISSIONS: [&str; 2] = [
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
    fn call_void(&self, name: &'static jni::strings::JNIStr) -> Result<(), String> {
        call_cranpose_activity_method(&self.app, HEART_RATE_CLASS, name)
    }
}

impl HeartRateMonitor for AndroidHeartRate {
    fn available(&self) -> bool {
        call_cranpose_class(&self.app, HEART_RATE_CLASS, |env, activity, class| {
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
        let answer = call_cranpose_class(&self.app, HEART_RATE_CLASS, |env, activity, class| {
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

#[cfg(test)]
#[path = "tests/android_heart_rate_tests.rs"]
mod tests;
