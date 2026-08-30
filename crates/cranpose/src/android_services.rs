#![allow(unsafe_code)]

use std::{
    rc::Rc,
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering},
    },
};

use cranpose_services::{
    AppUpdateCapabilities, AppUpdateError, AppUpdateStatus, AppUpdater, BackgroundActivity,
    BatteryStatus, BundledAssetError, BundledAssetReader, BundledAssets, GitHubReleaseUpdate,
    HapticEffect, HapticFeedback, HapticPattern, Haptics, IncomingContent, LaunchArgs,
    MemoryPressure, NetworkMonitor, NetworkStatus, Notifier, NotifyRequest, PackageDigest,
    PowerCapabilities, PowerMonitor, PowerReading, ShareContent, ShareError, ShareSheet,
    StreamingAssetReader, ThermalState, UpdatePackage, publish_incoming_content,
    publish_memory_pressure, push_notification_deeplink, set_platform_app_updater,
    set_platform_background_activity, set_platform_bundled_assets, set_platform_haptics,
    set_platform_launch_args, set_platform_network_monitor, set_platform_notifier,
    set_platform_power_monitor, set_platform_share_sheet,
};
use jni::{
    EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JByteArray, JClass, JObject, JString, JValue},
    sys::{jboolean, jint, jlong},
};

use crate::{
    android_haptics_queue::{HapticCommand, HapticQueue},
    android_jni::{clear_pending_android_jni_exception, with_android_activity_env},
    android_launch_args::decode_launch_arguments,
};

static NETWORK_ONLINE: AtomicBool = AtomicBool::new(true);
static NETWORK_METERED: AtomicBool = AtomicBool::new(false);
static NETWORK_CALLBACK_REGISTERED: AtomicBool = AtomicBool::new(false);

static INSETS_LEFT_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_TOP_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_RIGHT_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_BOTTOM_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_CHANGED: AtomicBool = AtomicBool::new(false);

static PENDING_LAUNCH_ARGS: Mutex<Option<String>> = Mutex::new(None);

static LOOP_WAKER: OnceLock<Mutex<Option<android_activity::AndroidAppWaker>>> = OnceLock::new();

fn loop_waker() -> &'static Mutex<Option<android_activity::AndroidAppWaker>> {
    LOOP_WAKER.get_or_init(|| Mutex::new(None))
}

pub(crate) fn wake_native_loop() {
    let waker = loop_waker().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(waker) = waker.as_ref() {
        waker.wake();
    }
}

pub(crate) fn register(app: android_activity::AndroidApp) {
    let mut waker = loop_waker().lock().unwrap_or_else(PoisonError::into_inner);
    *waker = Some(app.create_waker());
    drop(waker);
    let haptics_queue = if async_haptics_enabled() {
        spawn_haptics_thread(app.clone())
    } else {
        log::info!("CRANPOSE_ASYNC_HAPTICS=0: haptics stay on the calling thread");
        None
    };
    set_platform_haptics(Arc::new(AndroidHaptics {
        app: app.clone(),
        amplitude_control: AtomicU8::new(0),
        queue: haptics_queue,
    }));
    set_platform_share_sheet(Rc::new(AndroidShareSheet { app: app.clone() }));
    set_platform_notifier(Arc::new(AndroidNotifier { app: app.clone() }));
    set_platform_network_monitor(Arc::new(AndroidNetworkMonitor { app: app.clone() }));
    set_platform_power_monitor(Arc::new(AndroidPowerMonitor { app: app.clone() }));
    set_platform_background_activity(Arc::new(AndroidBackgroundActivity { app: app.clone() }));
    set_platform_bundled_assets(Arc::new(AndroidBundledAssets { app: app.clone() }));
    set_platform_app_updater(Arc::new(AndroidAppUpdater { app: app.clone() }));
    crate::android_camera::register(app.clone());
    #[cfg(feature = "media")]
    crate::android_media::register(app.clone());
    set_platform_launch_args(Rc::new(read_launch_arguments(&app)));
    #[cfg(feature = "playbilling")]
    {
        crate::android_purchases::register(app.clone());
    }
    #[cfg(feature = "audio")]
    {
        cranpose_audio::install();
    }
}

struct AndroidAppUpdater {
    app: android_activity::AndroidApp,
}

impl AppUpdater for AndroidAppUpdater {
    fn capabilities(&self) -> AppUpdateCapabilities {
        AppUpdateCapabilities {
            check: true,
            install: true,
        }
    }

    fn check(&self, source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
        with_android_activity_env(&self.app, |env, activity| {
            let repository = env
                .new_string(&source.repository)
                .map_err(|error| error.to_string())?;
            let current_version = env
                .new_string(&source.current_version)
                .map_err(|error| error.to_string())?;
            let asset_suffix = env
                .new_string(&source.asset_suffix)
                .map_err(|error| error.to_string())?;
            env.call_method(
                &activity,
                jni_str!("cranposeCheckGitHubUpdate"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V"),
                &[
                    JValue::Object(repository.as_ref()),
                    JValue::Object(current_version.as_ref()),
                    JValue::Object(asset_suffix.as_ref()),
                ],
            )
            .map(|_| ())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        })
        .map_err(AppUpdateError::Request)
    }

    fn install(&self, package: &UpdatePackage) -> Result<(), AppUpdateError> {
        let digest = package
            .digest
            .as_ref()
            .map(PackageDigest::to_feed_string)
            .unwrap_or_default();
        with_android_activity_env(&self.app, |env, activity| {
            let download_url = env
                .new_string(&package.download_url)
                .map_err(|error| error.to_string())?;
            let digest = env.new_string(&digest).map_err(|error| error.to_string())?;
            env.call_method(
                &activity,
                jni_str!("cranposeInstallUpdate"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;J)V"),
                &[
                    JValue::Object(download_url.as_ref()),
                    JValue::Object(digest.as_ref()),
                    JValue::Long(package.size.unwrap_or(0) as i64),
                ],
            )
            .map(|_| ())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        })
        .map_err(AppUpdateError::Request)
    }
}

struct AndroidBundledAssets {
    app: android_activity::AndroidApp,
}

impl BundledAssets for AndroidBundledAssets {
    fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
        let result = with_android_activity_env(&self.app, |env, activity| {
            let path_string = env.new_string(path).map_err(|error| error.to_string())?;
            let path_object: &JObject = path_string.as_ref();
            let value = env
                .call_method(
                    &activity,
                    jni_str!("cranposeReadBundledAsset"),
                    jni_sig!("(Ljava/lang/String;)[B"),
                    &[JValue::Object(path_object)],
                )
                .and_then(|value| value.l())
                .map_err(|error| {
                    clear_pending_android_jni_exception(env);
                    error.to_string()
                })?;
            if value.is_null() {
                return Ok(None);
            }
            let bytes = JByteArray::cast_local(env, value).map_err(|error| error.to_string())?;
            env.convert_byte_array(&bytes)
                .map(Some)
                .map_err(|error| error.to_string())
        })
        .map_err(|message| BundledAssetError::ReadFailed {
            path: path.to_string(),
            message,
        })?;
        result.ok_or_else(|| BundledAssetError::NotFound(path.to_string()))
    }

    fn open(&self, path: &str) -> Result<Box<dyn BundledAssetReader>, BundledAssetError> {
        match self.asset_descriptor(path) {
            Some(reader) => Ok(Box::new(reader)),
            None => Ok(Box::new(StreamingAssetReader::new(
                path,
                std::io::Cursor::new(self.read(path)?),
            ))),
        }
    }

    fn len(&self, path: &str) -> Option<u64> {
        let manager = self.app.asset_manager();
        let name = std::ffi::CString::new(path).ok()?;
        Some(manager.open(&name)?.length() as u64)
    }
}

impl AndroidBundledAssets {
    fn asset_descriptor(&self, path: &str) -> Option<StreamingAssetReader<std::fs::File>> {
        let manager = self.app.asset_manager();
        let name = std::ffi::CString::new(path).ok()?;
        let asset = manager.open(&name)?;
        let descriptor = asset.open_file_descriptor().ok()?;
        let mut file = std::fs::File::from(descriptor.fd);
        std::io::Seek::seek(
            &mut file,
            std::io::SeekFrom::Start(descriptor.offset as u64),
        )
        .ok()?;
        Some(StreamingAssetReader::with_length(
            path,
            file,
            descriptor.size as u64,
        ))
    }
}

struct AndroidBackgroundActivity {
    app: android_activity::AndroidApp,
}

impl BackgroundActivity for AndroidBackgroundActivity {
    fn set_active(&self, active: bool) {
        let result = with_android_activity_env(&self.app, |env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeSetBackgroundActive"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(active)],
            )
            .map(|_| ())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        });
        if let Err(error) = result {
            log::warn!("Android background activity set_active({active}) failed: {error}");
        }
    }
}

fn call_android_activity_int(
    app: &android_activity::AndroidApp,
    method: &'static jni::strings::JNIStr,
    fallback: i32,
) -> i32 {
    with_android_activity_env(app, |env, activity| {
        env.call_method(&activity, method, jni_sig!("()I"), &[])
            .and_then(|value| value.i())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
    })
    .unwrap_or(fallback)
}

fn call_android_activity_bool(
    app: &android_activity::AndroidApp,
    method: &'static jni::strings::JNIStr,
    fallback: bool,
) -> bool {
    with_android_activity_env(app, |env, activity| {
        env.call_method(&activity, method, jni_sig!("()Z"), &[])
            .and_then(|value| value.z())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
    })
    .unwrap_or(fallback)
}

struct AndroidPowerMonitor {
    app: android_activity::AndroidApp,
}

impl AndroidPowerMonitor {
    fn call_int(&self, method: &'static jni::strings::JNIStr, fallback: i32) -> i32 {
        call_android_activity_int(&self.app, method, fallback)
    }

    fn call_bool(&self, method: &'static jni::strings::JNIStr, fallback: bool) -> bool {
        call_android_activity_bool(&self.app, method, fallback)
    }
}

impl PowerMonitor for AndroidPowerMonitor {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            thermal: true,
            battery: true,
            background_restriction: true,
        }
    }

    fn thermal_state(&self) -> PowerReading<ThermalState> {
        PowerReading::Known(match self.call_int(jni_str!("cranposeThermalStatus"), 0) {
            i32::MIN..=0 => ThermalState::Normal,
            1 => ThermalState::Light,
            2 => ThermalState::Moderate,
            3 => ThermalState::Severe,
            4 => ThermalState::Critical,
            5 => ThermalState::Emergency,
            _ => ThermalState::Shutdown,
        })
    }

    fn battery_status(&self) -> PowerReading<BatteryStatus> {
        let packed = self.call_int(jni_str!("cranposeBatteryStatus"), 0x100 | 100);
        PowerReading::Known(BatteryStatus {
            percent: (packed & 0xff).clamp(0, 100) as u8,
            charging: (packed & 0x100) != 0,
        })
    }

    fn unrestricted_background_work(&self) -> PowerReading<bool> {
        PowerReading::Known(self.call_bool(jni_str!("cranposeUnrestrictedBackgroundWork"), true))
    }

    fn request_unrestricted_background_work(&self) {
        let _ = with_android_activity_env(&self.app, |env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeRequestUnrestrictedBackgroundWork"),
                jni_sig!("()V"),
                &[],
            )
            .map(|_| ())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        });
    }
}

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
            log::warn!("Android launch arguments unavailable: {error}");
            LaunchArgs::default()
        }
    }
}

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
        if crate::android::android_platform_env().set_safe_area(insets)
            && let Some(shell) = shell
        {
            shell.request_root_render();
        }
    }
}

const HAPTIC_QUEUE_CAPACITY: usize = 8;

fn async_haptics_enabled() -> bool {
    match std::env::var("CRANPOSE_ASYNC_HAPTICS") {
        Ok(value) => !matches!(value.trim(), "0" | "false" | "off" | "no"),
        Err(_) => true,
    }
}

struct AndroidHaptics {
    app: android_activity::AndroidApp,
    amplitude_control: AtomicU8,
    queue: Option<Arc<HapticQueue>>,
}

impl AndroidHaptics {
    fn dispatch(&self, command: HapticCommand) {
        let command = match &self.queue {
            Some(queue) => match queue.enqueue(command) {
                Ok(()) => return,
                Err(command) => command,
            },
            None => command,
        };
        deliver_haptic_command(&self.app, &command);
    }
}

impl Drop for AndroidHaptics {
    fn drop(&mut self) {
        if let Some(queue) = &self.queue {
            queue.shut_down();
        }
    }
}

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

fn run_haptics_thread(app: android_activity::AndroidApp, queue: Arc<HapticQueue>) {
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
        log::warn!("Android {what} failed: {error}");
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
        match self.amplitude_control.load(Ordering::Acquire) {
            1 => return false,
            2 => return true,
            _ => {}
        }
        let supported = call_android_activity_bool(
            &self.app,
            jni_str!("cranposeHapticHasAmplitudeControl"),
            false,
        );
        self.amplitude_control
            .store(if supported { 2 } else { 1 }, Ordering::Release);
        supported
    }
}

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

struct AndroidNetworkMonitor {
    app: android_activity::AndroidApp,
}

impl NetworkMonitor for AndroidNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        NetworkStatus {
            online: NETWORK_ONLINE.load(Ordering::Acquire),
            metered: NETWORK_METERED.load(Ordering::Acquire),
        }
    }

    fn is_alive(&self) -> bool {
        NETWORK_CALLBACK_REGISTERED.load(Ordering::Acquire)
    }

    fn reconnect(&self) {
        let registered =
            call_android_activity_bool(&self.app, jni_str!("cranposeEnsureNetworkCallback"), false);
        NETWORK_CALLBACK_REGISTERED.store(registered, Ordering::Release);
    }
}

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

#[doc(hidden)]
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnNetworkRegistration(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    registered: jboolean,
) {
    NETWORK_CALLBACK_REGISTERED.store(registered, Ordering::Release);
}

#[doc(hidden)]
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeNotificationAction<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    deeplink: JString<'local>,
) {
    let Some(deeplink) = crate::android_jni::decode_jni_string(&mut env, deeplink) else {
        return;
    };
    if !deeplink.is_empty() {
        push_notification_deeplink(deeplink);
        wake_native_loop();
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnIncomingContent<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    name: JString<'local>,
    mime_type: JString<'local>,
    uri: JString<'local>,
) {
    let shared = env.with_env(|env| -> jni::errors::Result<IncomingContent> {
        let name = name.try_to_string(env)?;
        let mime_type = mime_type.try_to_string(env)?;
        let uri = uri.try_to_string(env)?;
        let mut shared = IncomingContent::from_uri(uri);
        if !name.is_empty() {
            shared = shared.with_name(name);
        }
        if !mime_type.is_empty() {
            shared = shared.with_mime_type(mime_type);
        }
        Ok(shared)
    });
    match shared.into_outcome() {
        Outcome::Ok(shared) => {
            publish_incoming_content(shared);
            wake_native_loop();
        }
        Outcome::Err(_) | Outcome::Panic(_) => {
            log::warn!("incoming share: the strings did not decode");
        }
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnTrimMemory(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    level: jint,
) {
    publish_memory_pressure(MemoryPressure::from_android_trim_level(level));
    wake_native_loop();
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAppUpdateStatus<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    kind: jint,
    version: JString<'local>,
    download_url: JString<'local>,
    downloaded: jlong,
    total: jlong,
    message: JString<'local>,
    digest: JString<'local>,
) {
    let decoded = env.with_env(
        |env| -> jni::errors::Result<(String, String, String, String)> {
            Ok((
                version.try_to_string(env)?,
                download_url.try_to_string(env)?,
                message.try_to_string(env)?,
                digest.try_to_string(env)?,
            ))
        },
    );
    let Outcome::Ok((version, download_url, message, digest)) = decoded.into_outcome() else {
        return;
    };
    let status = match kind {
        1 => AppUpdateStatus::Checking,
        2 => AppUpdateStatus::UpToDate,
        3 => {
            let mut package = UpdatePackage::new(version, download_url);
            if total > 0 {
                package = package.with_size(total as u64);
            }
            if let Some(digest) = PackageDigest::parse(&digest) {
                package = package.with_digest(digest);
            }
            AppUpdateStatus::Available { package }
        }
        4 => AppUpdateStatus::Downloading {
            downloaded: downloaded.max(0) as u64,
            total: (total > 0).then_some(total as u64),
        },
        5 => AppUpdateStatus::AwaitingConfirmation,
        6 => AppUpdateStatus::Installing,
        7 => AppUpdateStatus::Error(if message.is_empty() {
            "application update failed".to_string()
        } else {
            message
        }),
        8 => AppUpdateStatus::Verifying,
        _ => AppUpdateStatus::Idle,
    };
    cranpose_services::set_app_update_status(status);
    wake_native_loop();
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnLaunchArguments<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    payload: JString<'local>,
) {
    let Some(payload) = crate::android_jni::decode_jni_string(&mut env, payload) else {
        return;
    };
    if let Ok(mut slot) = PENDING_LAUNCH_ARGS.lock() {
        *slot = Some(payload);
    }
    wake_native_loop();
}
