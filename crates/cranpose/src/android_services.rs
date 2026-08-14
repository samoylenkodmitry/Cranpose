//! Android backends for the cranpose service registry: haptics, share sheet,
//! notifications (with deep-links), network status, clipboard, window insets,
//! and the launching intent's extras — the JNI counterparts of the capability
//! hooks in `dev.cranpose.android.CranposeActivity`.
//!
//! Rust → Java goes through the activity over JNI (each Java method hops to
//! the UI thread itself). Java → Rust arrives on the Android UI thread via
//! the exported `Java_dev_cranpose_android_CranposeActivity_*` symbols below;
//! those must not touch the composition (which lives on the native-activity
//! thread), so they park values in atomics and wake the native loop, which
//! applies them via [`apply_pending_platform_signals`].
#![allow(unsafe_code)]

use crate::android_haptics_queue::{HapticCommand, HapticQueue};
use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};
use crate::android_launch_args::decode_launch_arguments;
use cranpose_services::{
    push_notification_deeplink, set_platform_haptics, set_platform_launch_args,
    set_platform_network_monitor, set_platform_notifier, set_platform_share_sheet, HapticEffect,
    HapticFeedback, HapticPattern, Haptics, LaunchArgs, NetworkMonitor, NetworkStatus, Notifier,
    NotifyRequest, ShareContent, ShareError, ShareSheet,
};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{jni_sig, jni_str, EnvUnowned, Outcome};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

// --- Cross-thread signal parking (UI thread → native loop) -------------------

static NETWORK_ONLINE: AtomicBool = AtomicBool::new(true);
static NETWORK_METERED: AtomicBool = AtomicBool::new(false);

static INSETS_LEFT_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_TOP_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_RIGHT_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_BOTTOM_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_CHANGED: AtomicBool = AtomicBool::new(false);

/// Re-encoded intent extras parked by `onNewIntent`, waiting for the native
/// loop to swap in the new snapshot.
static PENDING_LAUNCH_ARGS: Mutex<Option<String>> = Mutex::new(None);

/// Wakes the native event loop so parked signals are applied promptly.
static LOOP_WAKER: OnceLock<Mutex<Option<android_activity::AndroidAppWaker>>> = OnceLock::new();

pub(crate) fn wake_native_loop() {
    if let Some(waker) = LOOP_WAKER.get() {
        if let Ok(waker) = waker.lock() {
            if let Some(waker) = waker.as_ref() {
                waker.wake();
            }
        }
    }
}

/// Registers the Android service backends. Called once at startup with the
/// activity handle.
pub(crate) fn register(app: android_activity::AndroidApp) {
    let _ = LOOP_WAKER.set(Mutex::new(Some(app.create_waker())));
    let haptics_queue = if async_haptics_enabled() {
        spawn_haptics_thread(app.clone())
    } else {
        log::info!("CRANPOSE_ASYNC_HAPTICS=0: haptics stay on the calling thread");
        None
    };
    set_platform_haptics(Rc::new(AndroidHaptics {
        app: app.clone(),
        amplitude_control: Cell::new(None),
        queue: haptics_queue,
    }));
    set_platform_share_sheet(Rc::new(AndroidShareSheet { app: app.clone() }));
    set_platform_notifier(Rc::new(AndroidNotifier { app: app.clone() }));
    set_platform_network_monitor(Rc::new(AndroidNetworkMonitor));
    // Pulled rather than pushed from `onCreate`: `getIntent()` is populated
    // before the native thread starts, so reading it here is deterministic,
    // whereas a push would race the thread that consumes it.
    set_platform_launch_args(Rc::new(read_launch_arguments(&app)));
    #[cfg(feature = "playbilling")]
    {
        // Unlike audio, Play Billing needs the activity: the payment sheet is
        // an activity result, so the backend keeps the handle and passes it to
        // the Java bridge on every call.
        crate::android_purchases::register(app.clone());
    }
    #[cfg(feature = "audio")]
    {
        // AAudio is a pure-NDK API, so the engine needs nothing from the
        // activity. The output device itself opens on the first sound.
        cranpose_audio::install();
    }
}

/// Reads the launching intent's extras through `CranposeActivity`.
///
/// A launch without extras, or a host activity that is not a
/// `CranposeActivity`, yields empty arguments rather than an error: the app
/// still runs, it simply has nothing to read.
fn read_launch_arguments(app: &android_activity::AndroidApp) -> LaunchArgs {
    match with_android_activity_env(app, |env, activity| {
        let payload = env
            .call_method(
                &activity,
                jni_str!("cranposeEncodeLaunchArguments"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                format!("failed to read the Android launch arguments: {error}")
            })?;
        if payload.is_null() {
            return Ok(String::new());
        }
        JString::cast_local(env, payload)
            .and_then(|payload| payload.try_to_string(env))
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                format!("failed to decode the Android launch arguments: {error}")
            })
    }) {
        Ok(payload) => decode_launch_arguments(&payload),
        Err(error) => {
            // Worth a warning, not a debug line: an app whose debug/bench
            // flags silently read as absent is measurably indistinguishable
            // from one that ignores them, and that cost a device session once.
            log::warn!("Android launch arguments unavailable: {error}");
            LaunchArgs::default()
        }
    }
}

/// Applies signals parked by the UI-thread callbacks: window insets flow into
/// the platform environment (safe area) and a replacement launch intent swaps
/// in new launch arguments, forcing a root render when they changed. Called
/// from the native event loop.
pub(crate) fn apply_pending_platform_signals(
    density: f32,
    shell: &mut Option<cranpose_app_shell::AppShell<cranpose_render_wgpu::WgpuRenderer>>,
) {
    if let Some(payload) = PENDING_LAUNCH_ARGS
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
    {
        set_platform_launch_args(Rc::new(decode_launch_arguments(&payload)));
        if let Some(shell) = shell {
            shell.request_root_render();
        }
    }

    if INSETS_CHANGED.swap(false, Ordering::AcqRel) {
        let density = density.max(f32::EPSILON);
        let insets = cranpose_ui::EdgeInsets {
            left: INSETS_LEFT_PX.load(Ordering::Acquire) as f32 / density,
            top: INSETS_TOP_PX.load(Ordering::Acquire) as f32 / density,
            right: INSETS_RIGHT_PX.load(Ordering::Acquire) as f32 / density,
            bottom: INSETS_BOTTOM_PX.load(Ordering::Acquire) as f32 / density,
        };
        if crate::android::android_platform_env().set_safe_area(insets) {
            if let Some(shell) = shell {
                shell.request_root_render();
            }
        }
    }
}

// --- Haptics ------------------------------------------------------------------

/// Queue depth for the asynchronous dispatch. Small on purpose: at the
/// profiled rate of about one waveform per frame the queue only ever holds
/// what one slow binder call backs up, and past that waveforms coalesce.
const HAPTIC_QUEUE_CAPACITY: usize = 8;

/// Kill switch for the asynchronous dispatch: `CRANPOSE_ASYNC_HAPTICS=0`
/// (reachable on a device as `debug.cranpose.async_haptics` through the
/// property bridge in [`crate::android_frame_telemetry`]) keeps every
/// vibrator call on the calling thread, the pre-queue behaviour.
fn async_haptics_enabled() -> bool {
    match std::env::var("CRANPOSE_ASYNC_HAPTICS") {
        Ok(value) => !matches!(value.trim(), "0" | "false" | "off" | "no"),
        Err(_) => true,
    }
}

/// The Android vibrator, reached through `CranposeActivity`.
///
/// `perform` routes through `View.performHapticFeedback` so the OS's own
/// feedback settings apply. The amplitude, waveform and predefined-effect
/// paths address `Vibrator`/`VibrationEffect` directly, which is what a game
/// designing its own set of distinct feels needs; each one is a single JNI
/// call carrying primitives or a primitive array.
///
/// The vibrator methods run the `VibratorManagerService` binder call on the
/// calling thread, and a watch profile measured that at 0.88 ms per frame on
/// the render thread (946 waveform calls in 975 frames). Delivery therefore
/// normally happens on the dedicated "cranpose-haptics" thread: the trait
/// methods enqueue into `queue` and return immediately. When `queue` is
/// `None` (kill switch, or the thread failed to start) every call delivers
/// synchronously, exactly as before the queue existed.
struct AndroidHaptics {
    app: android_activity::AndroidApp,
    /// `Vibrator.hasAmplitudeControl()`, queried once and cached: the answer
    /// cannot change for the life of the process, and the query costs a JNI
    /// round trip that UI code should not repeat.
    amplitude_control: Cell<Option<bool>>,
    /// Handoff to the "cranpose-haptics" delivery thread, or `None` for the
    /// synchronous path.
    queue: Option<Arc<HapticQueue>>,
}

impl AndroidHaptics {
    fn dispatch(&self, command: HapticCommand) {
        let command = match &self.queue {
            Some(queue) => match queue.enqueue(command) {
                Ok(()) => return,
                // The delivery thread is gone (shutdown, or it panicked and
                // its exit guard closed the queue); deliver on this thread so
                // the call is not lost.
                Err(command) => command,
            },
            None => command,
        };
        deliver_haptic_command(&self.app, &command);
    }
}

impl Drop for AndroidHaptics {
    fn drop(&mut self) {
        // Lets the delivery thread drain what it already accepted and exit
        // cleanly. Not joined: drop runs on the render thread and the worker
        // may be inside a binder call.
        if let Some(queue) = &self.queue {
            queue.shut_down();
        }
    }
}

/// Starts the "cranpose-haptics" delivery thread, returning the queue the
/// backend enqueues into, or `None` when the thread could not start.
fn spawn_haptics_thread(app: android_activity::AndroidApp) -> Option<Arc<HapticQueue>> {
    let queue = Arc::new(HapticQueue::new(HAPTIC_QUEUE_CAPACITY));
    let worker_queue = Arc::clone(&queue);
    match std::thread::Builder::new()
        .name("cranpose-haptics".to_owned())
        .spawn(move || run_haptics_thread(app, worker_queue))
    {
        Ok(_) => Some(queue),
        Err(error) => {
            log::warn!(
                "cranpose-haptics thread failed to start; haptics stay synchronous: {error}"
            );
            None
        }
    }
}

/// Delivery loop for the "cranpose-haptics" thread.
///
/// The first `with_android_activity_env` call attaches the thread to the JVM
/// once (`attach_current_thread` only checks a thread-local after that), and
/// the attachment is released automatically when the thread exits after the
/// queue shuts down and drains — so the render thread pays neither the attach
/// nor the binder round trip.
fn run_haptics_thread(app: android_activity::AndroidApp, queue: Arc<HapticQueue>) {
    // Closes the queue even if delivery panics, so a caller blocked on a
    // saturated queue is released (and falls back to synchronous delivery)
    // instead of hanging the render thread forever.
    struct ShutDownOnExit(Arc<HapticQueue>);
    impl Drop for ShutDownOnExit {
        fn drop(&mut self) {
            self.0.shut_down();
        }
    }
    let _guard = ShutDownOnExit(Arc::clone(&queue));
    while let Some(command) = queue.dequeue() {
        deliver_haptic_command(&app, &command);
        queue.note_delivered();
    }
    log::debug!("cranpose-haptics thread exited after shutdown");
}

/// Forwards one haptic command to the activity — the single JNI path shared
/// by the delivery thread and the synchronous fallback, so the two cannot
/// drift apart.
fn deliver_haptic_command(app: &android_activity::AndroidApp, command: &HapticCommand) {
    let what = match command {
        HapticCommand::Perform(_) => "haptic",
        HapticCommand::OneShot { .. } => "haptic one-shot",
        HapticCommand::Waveform { .. } => "haptic waveform",
        HapticCommand::Effect(_) => "haptic effect",
        HapticCommand::Cancel => "haptic cancel",
    };
    let result = with_android_activity_env(app, |env, activity| match command {
        HapticCommand::Perform(feedback) => {
            let kind: jint = match feedback {
                HapticFeedback::ImpactLight | HapticFeedback::Selection => 0,
                HapticFeedback::ImpactMedium => 1,
                HapticFeedback::ImpactHeavy => 2,
                HapticFeedback::Success => 3,
                HapticFeedback::Warning | HapticFeedback::Error => 4,
            };
            env.call_method(
                &activity,
                jni_str!("cranposeHaptic"),
                jni_sig!("(I)V"),
                &[JValue::Int(kind)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        }
        HapticCommand::OneShot {
            duration_ms,
            amplitude,
        } => {
            // `VibrationEffect.createOneShot` takes DEFAULT_AMPLITUDE (-1) or
            // 1..=255; 0 means "device default" in the framework API, so it is
            // translated here rather than rejected by the platform.
            let amplitude: jint = if *amplitude == 0 {
                -1
            } else {
                jint::from(*amplitude)
            };
            env.call_method(
                &activity,
                jni_str!("cranposeHapticOneShot"),
                jni_sig!("(JI)V"),
                &[
                    JValue::Long(jlong::from(*duration_ms)),
                    JValue::Int(amplitude),
                ],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        }
        HapticCommand::Waveform {
            timings_ms,
            amplitudes,
            repeat,
        } => {
            let timing_array = env
                .new_long_array(timings_ms.len())
                .map_err(|error| error.to_string())?;
            timing_array
                .set_region(env, 0, timings_ms)
                .map_err(|error| error.to_string())?;
            let amplitude_array = env
                .new_int_array(amplitudes.len())
                .map_err(|error| error.to_string())?;
            amplitude_array
                .set_region(env, 0, amplitudes)
                .map_err(|error| error.to_string())?;
            let timing_obj: &JObject = timing_array.as_ref();
            let amplitude_obj: &JObject = amplitude_array.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeHapticWaveform"),
                jni_sig!("([J[II)V"),
                &[
                    JValue::Object(timing_obj),
                    JValue::Object(amplitude_obj),
                    JValue::Int(*repeat),
                ],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        }
        HapticCommand::Effect(effect) => {
            // Matches `VibrationEffect.EFFECT_*`, which the activity re-maps
            // by name so the constants stay owned by the platform.
            let id: jint = match effect {
                HapticEffect::Click => 0,
                HapticEffect::DoubleClick => 1,
                HapticEffect::Tick => 2,
                HapticEffect::HeavyClick => 3,
            };
            env.call_method(
                &activity,
                jni_str!("cranposeHapticPredefined"),
                jni_sig!("(I)V"),
                &[JValue::Int(id)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        }
        HapticCommand::Cancel => {
            env.call_method(
                &activity,
                jni_str!("cranposeHapticCancel"),
                jni_sig!("()V"),
                &[],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        }
    });
    if let Err(error) = result {
        log::debug!("Android {what} failed: {error}");
    }
}

impl Haptics for AndroidHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        self.dispatch(HapticCommand::Perform(feedback));
    }

    fn vibrate(&self, duration_ms: u32, amplitude: u8) {
        if duration_ms == 0 {
            return;
        }
        self.dispatch(HapticCommand::OneShot {
            duration_ms,
            amplitude,
        });
    }

    fn play_pattern(&self, pattern: &HapticPattern) {
        self.dispatch(HapticCommand::Waveform {
            timings_ms: pattern
                .timings_ms()
                .iter()
                .map(|step| i64::from(*step))
                .collect(),
            amplitudes: pattern
                .amplitudes()
                .iter()
                .map(|level| i32::from(*level))
                .collect(),
            repeat: pattern
                .repeat()
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(-1),
        });
    }

    fn perform_effect(&self, effect: HapticEffect) {
        self.dispatch(HapticCommand::Effect(effect));
    }

    fn cancel(&self) {
        self.dispatch(HapticCommand::Cancel);
    }

    fn has_amplitude_control(&self) -> bool {
        if let Some(known) = self.amplitude_control.get() {
            return known;
        }
        let supported = with_android_activity_env(&self.app, |env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeHapticHasAmplitudeControl"),
                jni_sig!("()Z"),
                &[],
            )
            .and_then(|value| value.z())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        })
        .unwrap_or(false);
        self.amplitude_control.set(Some(supported));
        supported
    }
}

// --- Share sheet ----------------------------------------------------------------

struct AndroidShareSheet {
    app: android_activity::AndroidApp,
}

impl ShareSheet for AndroidShareSheet {
    fn share(&self, content: ShareContent) -> Result<(), ShareError> {
        with_android_activity_env(&self.app, |env, activity| {
            let name = env
                .new_string(&content.file_name)
                .map_err(|error| error.to_string())?;
            let mime = env
                .new_string(&content.mime_type)
                .map_err(|error| error.to_string())?;
            let bytes = env
                .byte_array_from_slice(&content.bytes)
                .map_err(|error| error.to_string())?;
            let text = env
                .new_string(content.text.as_deref().unwrap_or(""))
                .map_err(|error| error.to_string())?;
            let name_obj: &JObject = name.as_ref();
            let mime_obj: &JObject = mime.as_ref();
            let bytes_obj: &JObject = bytes.as_ref();
            let text_obj: &JObject = text.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeShare"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;[BLjava/lang/String;)V"),
                &[
                    JValue::Object(name_obj),
                    JValue::Object(mime_obj),
                    JValue::Object(bytes_obj),
                    JValue::Object(text_obj),
                ],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        })
        .map_err(ShareError::Failed)
    }

    fn is_supported(&self) -> bool {
        true
    }
}

// --- Notifier -------------------------------------------------------------------

struct AndroidNotifier {
    app: android_activity::AndroidApp,
}

impl AndroidNotifier {
    fn call(&self, run: impl FnOnce(&mut jni::Env<'_>, JObject<'_>) -> Result<(), String>) {
        let result = with_android_activity_env(&self.app, |env, activity| run(env, activity));
        if let Err(error) = result {
            log::warn!("Android notifier call failed: {error}");
        }
    }
}

impl Notifier for AndroidNotifier {
    fn request_permission(&self) {
        self.call(|env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeNotifyRequestPermission"),
                jni_sig!("()V"),
                &[],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }

    fn notify(&self, request: NotifyRequest) {
        self.call(|env, activity| {
            let tag = env
                .new_string(&request.id)
                .map_err(|error| error.to_string())?;
            let title = env
                .new_string(&request.title)
                .map_err(|error| error.to_string())?;
            let body = env
                .new_string(&request.body)
                .map_err(|error| error.to_string())?;
            let deeplink = env
                .new_string(request.deeplink.as_deref().unwrap_or(""))
                .map_err(|error| error.to_string())?;
            let tag_obj: &JObject = tag.as_ref();
            let title_obj: &JObject = title.as_ref();
            let body_obj: &JObject = body.as_ref();
            let deeplink_obj: &JObject = deeplink.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeNotify"),
                jni_sig!(
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZLjava/lang/String;)V"
                ),
                &[
                    JValue::Object(tag_obj),
                    JValue::Object(title_obj),
                    JValue::Object(body_obj),
                    JValue::Bool(request.ongoing),
                    JValue::Object(deeplink_obj),
                ],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }

    fn cancel(&self, id: &str) {
        self.call(|env, activity| {
            let tag = env.new_string(id).map_err(|error| error.to_string())?;
            let tag_obj: &JObject = tag.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeNotifyCancel"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(tag_obj)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }
}

// --- Network monitor --------------------------------------------------------------

struct AndroidNetworkMonitor;

impl NetworkMonitor for AndroidNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        NetworkStatus {
            online: NETWORK_ONLINE.load(Ordering::Acquire),
            metered: NETWORK_METERED.load(Ordering::Acquire),
        }
    }
}

// --- Clipboard ---------------------------------------------------------------------

/// The Android system clipboard for the text-selection menu / `ClipboardManager`.
pub(crate) struct AndroidClipboard {
    pub(crate) app: android_activity::AndroidApp,
}

impl cranpose_ui::clipboard_session::PlatformClipboard for AndroidClipboard {
    fn write_text(&self, text: &str) {
        let result = with_android_activity_env(&self.app, |env, activity| {
            let value = env.new_string(text).map_err(|error| error.to_string())?;
            let value_obj: &JObject = value.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeClipboardSet"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(value_obj)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
        if let Err(error) = result {
            log::warn!("Android clipboard write failed: {error}");
        }
    }

    fn read_text(&self) -> Option<String> {
        with_android_activity_env(&self.app, |env, activity| {
            let value = env
                .call_method(
                    &activity,
                    jni_str!("cranposeClipboardGet"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )
                .and_then(|value| value.l())
                .map_err(|error| {
                    clear_pending_android_jni_exception(env);
                    error.to_string()
                })?;
            let value = env
                .cast_local::<JString>(value)
                .map_err(|error| error.to_string())?;
            value.try_to_string(env).map_err(|error| error.to_string())
        })
        .ok()
        .filter(|text| !text.is_empty())
    }
}

// --- Java → Rust callbacks (Android UI thread) ---------------------------------------

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnNetworkStatus(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    online: jboolean,
    metered: jboolean,
) {
    NETWORK_ONLINE.store(online, Ordering::Release);
    NETWORK_METERED.store(metered, Ordering::Release);
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnInsetsChanged(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    left: jint,
    top: jint,
    right: jint,
    bottom: jint,
) {
    INSETS_LEFT_PX.store(left, Ordering::Release);
    INSETS_TOP_PX.store(top, Ordering::Release);
    INSETS_RIGHT_PX.store(right, Ordering::Release);
    INSETS_BOTTOM_PX.store(bottom, Ordering::Release);
    INSETS_CHANGED.store(true, Ordering::Release);
    wake_native_loop();
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeNotificationAction<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    deeplink: JString<'local>,
) {
    let deeplink = match env
        .with_env(|env| -> jni::errors::Result<String> { deeplink.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(deeplink) => deeplink,
        Outcome::Err(_) | Outcome::Panic(_) => return,
    };
    if !deeplink.is_empty() {
        push_notification_deeplink(deeplink);
        wake_native_loop();
    }
}

/// A replacement launch intent arrived. The extras are parked and applied by
/// the native loop, which is the thread that owns the launch-argument
/// snapshot; the payload replaces the previous one wholesale, matching
/// `setIntent` replacing what a Compose activity reads from `getIntent`.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnLaunchArguments<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    payload: JString<'local>,
) {
    let payload = match env
        .with_env(|env| -> jni::errors::Result<String> { payload.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(payload) => payload,
        Outcome::Err(_) | Outcome::Panic(_) => return,
    };
    if let Ok(mut slot) = PENDING_LAUNCH_ARGS.lock() {
        *slot = Some(payload);
    }
    wake_native_loop();
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFileSaved<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    ok: jboolean,
    error: JString<'local>,
) {
    let error = match env
        .with_env(|env| -> jni::errors::Result<String> { error.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(error) if !error.is_empty() => Some(error),
        _ => None,
    };
    crate::android_file_picker::resolve_pending_save(token, ok, error);
    wake_native_loop();
}
