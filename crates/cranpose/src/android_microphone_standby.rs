#![expect(unsafe_code)]

//! The microphone standby on Android, through `CranposeMicrophoneStandby`,
//! which an application carries only when it declares the
//! `microphone-standby` service in its Gradle build. The Stop action of its
//! notification comes back through `nativeOnStandbyStop`.

use std::sync::Arc;

use cranpose_services::{
    MicrophoneStandby, StandbyNotice, microphone_standby_stopped, set_platform_microphone_standby,
};
use jni::{
    EnvUnowned, jni_sig, jni_str,
    objects::{JClass, JValue},
};

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

    fn set_notice(&self, notice: &StandbyNotice) {
        let result = self
            .context
            .call_class(STANDBY_CLASS, |env, context, class| {
                let text = env.new_string(&notice.text)?;
                let stop = env.new_string(&notice.stop)?;
                env.call_static_method(
                    class,
                    jni_str!("setNotice"),
                    jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V"),
                    &[
                        JValue::Object(context),
                        JValue::Object(text.as_ref()),
                        JValue::Object(stop.as_ref()),
                    ],
                )
                .map(|_| ())
            });
        if let Err(error) = result {
            log::warn!("the microphone standby's notice did not change: {error}");
        }
    }
}

/// The person ended the standby with its notification's Stop action.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeMicrophoneStandby_nativeOnStandbyStop(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) {
    microphone_standby_stopped();
}
