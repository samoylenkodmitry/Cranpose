//! The microphone standby on Android, through `CranposeMicrophoneStandby`,
//! which an application carries only when it declares the
//! `microphone-standby` service in its Gradle build.

use std::sync::Arc;

use cranpose_services::{MicrophoneStandby, set_platform_microphone_standby};
use jni::{jni_sig, jni_str, objects::JValue};

use crate::android_jni::AndroidContext;

const STANDBY_CLASS: &str = "dev/cranpose/android/CranposeMicrophoneStandby";

pub(crate) fn register(context: &'static AndroidContext) {
    if context.call_class(STANDBY_CLASS, |_, _, _| Ok(())).is_ok() {
        set_platform_microphone_standby(Some(Arc::new(AndroidMicrophoneStandby { context })));
    }
}

struct AndroidMicrophoneStandby {
    context: &'static AndroidContext,
}

impl MicrophoneStandby for AndroidMicrophoneStandby {
    fn set_held(&self, held: bool) {
        let name = if held {
            jni_str!("start")
        } else {
            jni_str!("stop")
        };
        let result = self
            .context
            .call_class(STANDBY_CLASS, |env, context, class| {
                env.call_static_method(
                    class,
                    name,
                    jni_sig!("(Landroid/content/Context;)V"),
                    &[JValue::Object(context)],
                )
                .map(|_| ())
            });
        if let Err(error) = result {
            log::warn!("the microphone standby did not change: {error}");
        }
    }
}
