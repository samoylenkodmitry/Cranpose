//! What an Android process starts with, whichever part of the application
//! the system started it for.
//!
//! The system starts the process for the activity, and also for a service
//! alone: Google Play services starts it to hand over what a paired watch
//! sends. A process started for a service has no activity, so only what needs
//! no window starts here: the logger, the application's folders, the services
//! that work from the application context, and the application's own
//! [`AppLauncher::with_process_start`](crate::AppLauncher::with_process_start).

use std::sync::{Once, OnceLock};

use jni::{
    EnvUnowned, Outcome,
    objects::{JClass, JObject},
};

use crate::{AppSettings, android_jni::AndroidContext};

static CONTEXT: OnceLock<AndroidContext> = OnceLock::new();

/// Starts the process once, with the context `context` gives, and then the
/// application's process start.
fn start(settings: &AppSettings, context: impl FnOnce() -> Result<AndroidContext, String>) {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        crate::android::init_logging(
            settings
                .log_tag
                .as_deref()
                .unwrap_or(crate::android::DEFAULT_LOG_TAG),
        );
        match context() {
            Ok(context) => register(CONTEXT.get_or_init(|| context)),
            Err(error) => log::error!("the process started without its services: {error}"),
        }
    });
    settings.start_process();
}

fn register(context: &'static AndroidContext) {
    crate::android_host::install_process(context);
    // Present only when the application declares the `wearable` service in
    // its Gradle build; without it the link reports itself unavailable.
    crate::android_wearable::register(context);
    // Present only when the application declares `microphone-standby`.
    crate::android_microphone_standby::register(context);
}

/// Starts the process for the activity `app` runs, if no service started it.
pub(crate) fn start_for_activity(app: &android_activity::AndroidApp, settings: &AppSettings) {
    start(settings, || AndroidContext::of_activity(app));
}

/// Starts the process for a service, from `CranposeProcess.nativeStart`.
pub(crate) fn start_for_service(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    context: JObject<'_>,
    settings: &AppSettings,
) {
    let started = env.with_env(|env| -> jni::errors::Result<()> {
        start(settings, || AndroidContext::new(env, &context));
        Ok(())
    });
    if let Outcome::Err(_) | Outcome::Panic(_) = started.into_outcome() {
        log::error!("the process start for a service failed");
    }
}
