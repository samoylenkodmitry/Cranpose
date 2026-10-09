//! Permission to record on Android, through `CranposeMicrophone`, which an
//! application carries only when its build script declares
//! `Use::microphone`. The answer comes back through `android_permissions`.

use std::sync::Arc;

use cranpose_services::{MicrophoneAccess, MicrophonePermission, set_platform_microphone_access};
use jni::{jni_sig, jni_str, objects::JValue};

use crate::android_jni::{call_cranpose_activity_method, call_cranpose_class};

const MICROPHONE_CLASS: &str = "dev/cranpose/android/CranposeMicrophone";

/// The permission recording needs.
pub(crate) const MICROPHONE_PERMISSION: &str = "android.permission.RECORD_AUDIO";

pub(crate) fn register(app: android_activity::AndroidApp) {
    set_platform_microphone_access(Some(Arc::new(AndroidMicrophone { app })));
}

struct AndroidMicrophone {
    app: android_activity::AndroidApp,
}

impl MicrophoneAccess for AndroidMicrophone {
    fn permission(&self) -> MicrophonePermission {
        let answer = call_cranpose_class(&self.app, MICROPHONE_CLASS, |env, activity, class| {
            env.call_static_method(
                class,
                jni_str!("cranposeMicrophonePermission"),
                jni_sig!("(Landroid/app/Activity;)I"),
                &[JValue::Object(activity)],
            )
            .and_then(jni::JValueOwned::i)
        });
        match answer {
            Ok(1) => MicrophonePermission::Granted,
            Ok(2) => MicrophonePermission::Denied,
            Ok(3) => MicrophonePermission::Blocked,
            Ok(_) => MicrophonePermission::NotAsked,
            Err(error) => {
                log::warn!("cranpose: the microphone permission could not be read: {error}");
                MicrophonePermission::NotAsked
            }
        }
    }

    fn request(&self) {
        if let Err(error) = call_cranpose_activity_method(
            &self.app,
            MICROPHONE_CLASS,
            jni_str!("cranposeMicrophoneRequestPermission"),
        ) {
            log::warn!("cranpose: the microphone permission was not requested: {error}");
        }
    }

    fn open_settings(&self) {
        if let Err(error) = call_cranpose_activity_method(
            &self.app,
            MICROPHONE_CLASS,
            jni_str!("cranposeMicrophoneOpenSettings"),
        ) {
            log::warn!("cranpose: the application settings did not open: {error}");
        }
    }
}
