#![expect(unsafe_code, reason = "JNI calls into the Android system")]

//! Asking the owner to unlock on Android, through `CranposeDeviceAuth`,
//! which an application carries only when its build script declares
//! `Use::device_auth`. Android 10 and newer show `BiometricPrompt` with the
//! screen lock allowed; Android 9 and older show the confirm-credential
//! screen, whose answer comes back through `CranposeActivity`'s activity
//! results.

use std::sync::{Arc, Mutex, PoisonError};

use cranpose_services::{
    DeviceAuthAvailability, DeviceAuthOutcome, DeviceAuthRequest, DeviceAuthenticator,
    finish_device_auth, set_platform_device_authenticator,
};
use jni::{
    EnvUnowned, jni_sig, jni_str,
    objects::{JClass, JObject, JValue},
    sys::{jint, jlong},
};

use super::{
    device_auth_wire::{CONFIRM_CREDENTIAL_REQUEST, availability, confirmed, outcome},
    jni::call_cranpose_class,
};

const DEVICE_AUTH_CLASS: &str = "dev/cranpose/android/CranposeDeviceAuth";

// The request whose prompt is open, which the confirm-credential screen's
// activity result does not name.
static OPEN: Mutex<Option<DeviceAuthRequest>> = Mutex::new(None);

pub(crate) fn register(app: android_activity::AndroidApp) {
    set_platform_device_authenticator(Some(Arc::new(AndroidDeviceAuth { app })));
}

struct AndroidDeviceAuth {
    app: android_activity::AndroidApp,
}

impl DeviceAuthenticator for AndroidDeviceAuth {
    fn availability(&self) -> DeviceAuthAvailability {
        let code = call_cranpose_class(&self.app, DEVICE_AUTH_CLASS, |env, activity, class| {
            env.call_static_method(
                class,
                jni_str!("cranposeDeviceAuthAvailability"),
                jni_sig!("(Landroid/app/Activity;)I"),
                &[JValue::Object(activity)],
            )
            .and_then(jni::JValueOwned::i)
        });
        match code {
            Ok(code) => availability(code),
            Err(error) => {
                log::warn!("cranpose: the device unlock could not be read: {error}");
                DeviceAuthAvailability::Unsupported
            }
        }
    }

    fn authenticate(&self, request: DeviceAuthRequest, reason: &str) {
        *OPEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(request);
        let shown = call_cranpose_class(&self.app, DEVICE_AUTH_CLASS, |env, activity, class| {
            let reason = env.new_string(reason)?;
            let reason: &JObject = reason.as_ref();
            env.call_static_method(
                class,
                jni_str!("cranposeDeviceAuthAuthenticate"),
                jni_sig!("(Landroid/app/Activity;JLjava/lang/String;)V"),
                &[
                    JValue::Object(activity),
                    // The id travels as a Java long and comes back unchanged.
                    JValue::Long(request.id().cast_signed()),
                    JValue::Object(reason),
                ],
            )
            .and_then(jni::JValueOwned::v)
        });
        if let Err(error) = shown {
            log::warn!("cranpose: the device unlock prompt did not open: {error}");
            finish(request, DeviceAuthOutcome::Failed);
        }
    }
}

fn finish(request: DeviceAuthRequest, outcome: DeviceAuthOutcome) {
    let mut open = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
    if *open == Some(request) {
        *open = None;
    }
    drop(open);
    finish_device_auth(request, outcome);
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeDeviceAuth_nativeOnDeviceAuthResult(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    request: jlong,
    kind: jint,
    code: jint,
) {
    finish(
        DeviceAuthRequest::from_id(request.cast_unsigned()),
        outcome(kind, code),
    );
}

// Every activity result `CranposeActivity` did not ask for itself. Only the
// confirm-credential screen's is the unlock's; the rest belong to no one
// in Rust.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnActivityResult(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    request_code: jint,
    result_code: jint,
) {
    if request_code != CONFIRM_CREDENTIAL_REQUEST {
        return;
    }
    let open = OPEN.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(request) = open {
        finish_device_auth(request, confirmed(result_code));
    }
}
