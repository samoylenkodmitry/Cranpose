//! What an Android process starts with, whichever part of the application
//! the system started it for.
//!
//! The system starts the process for the activity, and also for a service
//! alone: Google Play services starts it to hand over what a paired watch
//! sends. A process started for a service has no activity, so only what needs
//! no window starts here: the logger, the application's folders, the services
//! that work from the application context, and the application's own
//! [`AppLauncher::with_process_start`](crate::AppLauncher::with_process_start).
#![expect(unsafe_code, reason = "JNI and NDK calls into the Android system")]

use std::sync::{Once, OnceLock};

use jni::{
    EnvUnowned, JavaVM, Outcome,
    objects::{JClass, JObject},
};

use super::jni::AndroidContext;
use crate::AppSettings;

static CONTEXT: OnceLock<AndroidContext> = OnceLock::new();

/// Starts the process once, with the context `context` gives, and then the
/// application's process start.
fn start(settings: &AppSettings, context: impl FnOnce() -> Result<AndroidContext, String>) {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        super::init_logging(
            settings
                .log_tag
                .as_deref()
                .unwrap_or(super::DEFAULT_LOG_TAG),
        );
        install_panic_log();
        match context() {
            Ok(context) => register(CONTEXT.get_or_init(|| context)),
            Err(error) => log::error!("the process started without its services: {error}"),
        }
    });
    settings.start_process();
}

/// Logs a panic with its place and backtrace before the process ends, which
/// Android's stderr would drop.
fn install_panic_log() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(super::panic_hook::chained_panic_hook(
        |panic_info| {
            let location = panic_info.location().map_or_else(
                || "unknown location".to_string(),
                |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
            );
            let message = panic_info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| {
                    panic_info
                        .payload()
                        .downcast_ref::<String>()
                        .map(String::as_str)
                })
                .unwrap_or("Box<dyn Any>");
            let backtrace = std::backtrace::Backtrace::force_capture();
            log::error!("PANIC at {location}: {message}\n{backtrace}");
        },
        previous_hook,
    ));
}

fn register(context: &'static AndroidContext) {
    super::host::install_process(context);
    // Present only when the application declares the `wearable` service in
    // its Gradle build; without it the link reports itself unavailable.
    super::wearable::register(context);
    // Present only when the application declares `microphone-standby`.
    super::microphone_standby::register(context);
}

/// Starts the process for the activity `app` runs, if no service started it.
pub(crate) fn start_for_activity(app: &android_activity::AndroidApp, settings: &AppSettings) {
    start(settings, || AndroidContext::of_activity(app));
}

/// Starts the process for a service, from `CranposeProcess.nativeStart`.
///
/// With `share` the process has no activity, and the context is also set
/// where crates such as cpal read it, which the activity's start sets
/// otherwise. `CranposeProcess.beforeActivity` hands it back with
/// `nativeYieldContext` before an activity sets it.
pub(crate) fn start_for_service(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    context: JObject<'_>,
    share: bool,
    settings: &AppSettings,
) {
    let started = env.with_env(|env| -> jni::errors::Result<()> {
        start(settings, || {
            let context = AndroidContext::new(env, &context)?;
            if share {
                share_with_ndk(&context)?;
            }
            Ok(context)
        });
        Ok(())
    });
    if let Outcome::Err(_) | Outcome::Panic(_) = started.into_outcome() {
        log::error!("the process start for a service failed");
    }
}

fn share_with_ndk(context: &AndroidContext) -> Result<(), String> {
    let vm = JavaVM::singleton().map_err(|error| error.to_string())?;
    // SAFETY: the VM and the global reference to the application context
    // live as long as the process; Java calls this only where no activity
    // set the context, and hands it back before one does.
    unsafe {
        ndk_context::initialize_android_context(vm.get_raw().cast(), context.raw().cast());
    }
    Ok(())
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeProcess_nativeYieldContext(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) {
    // SAFETY: Java calls this once, on the main thread, only after a start
    // that set the context.
    unsafe { ndk_context::release_android_context() };
}
