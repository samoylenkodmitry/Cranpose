//! The microphone standby on Android, through `CranposeMicrophoneStandby`,
//! which an application carries only when it declares the
//! `microphone-standby` service in its Gradle build.

use std::sync::Arc;

use cranpose_services::{MicrophoneStandby, set_platform_microphone_standby};
use jni::{jni_sig, jni_str, objects::JValue};

use crate::android_jni::{
    clear_pending_android_jni_exception, load_cranpose_java_class, with_android_activity_env,
};

const STANDBY_CLASS: &str = "dev/cranpose/android/CranposeMicrophoneStandby";

pub(crate) fn register(app: android_activity::AndroidApp) {
    let declared = with_android_activity_env(&app, |env, activity| {
        load_cranpose_java_class(env, &activity, STANDBY_CLASS).map(|_| ())
    });
    if declared.is_ok() {
        set_platform_microphone_standby(Some(Arc::new(AndroidMicrophoneStandby { app })));
    }
}

struct AndroidMicrophoneStandby {
    app: android_activity::AndroidApp,
}

impl MicrophoneStandby for AndroidMicrophoneStandby {
    fn set_held(&self, held: bool) {
        let name = if held {
            jni_str!("start")
        } else {
            jni_str!("stop")
        };
        let result = with_android_activity_env(&self.app, |env, activity| {
            let class = load_cranpose_java_class(env, &activity, STANDBY_CLASS)?;
            env.call_static_method(
                class,
                name,
                jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .map(|_| ())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        });
        if let Err(error) = result {
            log::warn!("the microphone standby did not change: {error}");
        }
    }
}
