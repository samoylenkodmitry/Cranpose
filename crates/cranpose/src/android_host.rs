//! Android implementation of the framework host services.
#![allow(unsafe_code)]
use std::sync::Arc;

use cranpose_services::{
    set_application_id, set_host_controller, HostController, PlatformDirectories,
};
use jni::{jni_sig, jni_str, EnvUnowned};

use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};

struct AndroidHost {
    app: android_activity::AndroidApp,
}
impl HostController for AndroidHost {
    fn set_keep_screen_on(&self, enabled: bool) {
        let _ = with_android_activity_env(&self.app, |env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeSetKeepScreenOn"),
                jni_sig!("(Z)V"),
                &[enabled.into()],
            )
            .map(|_| ())
            .map_err(|e| {
                clear_pending_android_jni_exception(env);
                e.to_string()
            })
        });
    }
    fn platform_directories(&self) -> Option<PlatformDirectories> {
        let data = self.app.internal_data_path()?.to_path_buf();
        let sandbox = data.parent().unwrap_or(&data);
        Some(PlatformDirectories {
            data: data.clone(),
            config: data.join("config"),
            cache: sandbox.join("cache"),
            documents: Some(sandbox.join("files").join("documents")),
            temporary: sandbox.join("cache").join("temporary"),
            shared: self.app.external_data_path(),
        })
    }
    fn exit(&self) {
        let _ = crate::android_finish::finish_activity(&self.app);
    }
    fn background(&self) {
        let _ = with_android_activity_env(&self.app, |env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeMoveToBackground"),
                jni_sig!("()V"),
                &[],
            )
            .map(|_| ())
            .map_err(|e| {
                clear_pending_android_jni_exception(env);
                e.to_string()
            })
        });
    }

    fn durable_save_deadline(&self) -> std::time::Duration {
        // `onPause` is the last callback guaranteed before the process may be
        // killed, and it is on the critical path of the transition, so the
        // budget is short; overruns keep running under the background-work
        // lease the foreground service holds.
        std::time::Duration::from_secs(2)
    }
}

/// Reads the packaged application id (the Android package name).
fn package_name(app: &android_activity::AndroidApp) -> Option<String> {
    with_android_activity_env(app, |env, activity| {
        let value = env
            .call_method(
                &activity,
                jni_str!("getPackageName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
        if value.is_null() {
            return Ok(None);
        }
        jni::objects::JString::cast_local(env, value)
            .map_err(|error| error.to_string())?
            .try_to_string(env)
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .ok()
    .flatten()
}

pub(crate) fn install(app: android_activity::AndroidApp) {
    if let Some(path) = app.internal_data_path() {
        std::env::set_var("XDG_DATA_HOME", path);
    }
    if let Some(package) = package_name(&app) {
        if let Err(error) = set_application_id(&package) {
            log::warn!(
                "cranpose: the Android package name is not a usable application id: {error}"
            );
        }
    }
    set_host_controller(Arc::new(AndroidHost { app }));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnBackInvoked(
    _env: EnvUnowned<'_>,
    _class: jni::objects::JClass<'_>,
) -> jni::sys::jboolean {
    if cranpose_services::back_interception_enabled() {
        cranpose_services::push_back_request();
        true
    } else {
        false
    }
}
