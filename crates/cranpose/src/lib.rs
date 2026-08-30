#![deny(missing_docs)]

//! High level utilities for running Cranpose applications with minimal boilerplate.

#[cfg(all(feature = "android", target_os = "android"))]
mod android_file_picker;
#[cfg(any(
    all(feature = "android", target_os = "android"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
mod chunked_read;
mod scoped_weak_stack;
#[cfg(all(feature = "android", target_os = "android"))]
pub use android_file_picker::open_content_uri;
#[cfg(any(
    test,
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
mod accessibility;
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
mod accessibility_publish_policy;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_accessibility;
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
mod android_accessibility_wire;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_app_info;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_camera;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_display;
mod android_entry;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_finish;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_font_scale;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_frame_rate;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_frame_telemetry;
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
mod android_haptics_queue;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_host;
#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
mod android_host_window;
mod android_input;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_jni;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_keyboard;
#[cfg(any(test, all(feature = "android", target_os = "android")))]
mod android_launch_args;
#[cfg(all(feature = "android", feature = "media", target_os = "android"))]
mod android_media;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_overlay_window;
#[cfg(any(test, all(feature = "android", target_os = "android")))]
mod android_panic_hook;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_perf_hint;
#[cfg(any(
    test,
    all(feature = "android", feature = "playbilling", target_os = "android")
))]
mod android_purchase_wire;
#[cfg(all(
    feature = "android",
    feature = "playbilling",
    feature = "renderer-wgpu",
    target_os = "android"
))]
mod android_purchases;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_services;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_surface;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_text_input;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_vsync;
#[cfg(any(test, all(feature = "android", target_os = "android")))]
mod android_wire_escape;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_writable_folder;
mod app_launcher;
mod host_environment;
#[cfg(all(feature = "ios", target_os = "ios"))]
mod ios_host;
mod native_window;
/// The activity handle `NativeActivity` hands to the entry point. Re-exported so
/// an application declares its entry point with [`android_main!`] and never
/// depends on `android_activity` for a parameter type.
#[cfg(all(feature = "android", target_os = "android"))]
pub use android_activity::AndroidApp;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
pub use android_host_window::{
    AndroidHostWindowPositionError, AndroidHostWindowSizeError, AndroidHostWindowSizeStatus,
    AndroidHostWindowState, rememberAndroidHostWindowState,
};
#[cfg(all(
    feature = "renderer-wgpu",
    any(feature = "desktop-shell", all(feature = "ios", target_os = "ios"))
))]
pub use app_launcher::LaunchError;
pub use app_launcher::{AndroidOverlayWindowOptions, AppLauncher, AppSettings};
/// Font registration vocabulary named by [`AppLauncher`]'s font methods:
/// the platform font directory [`AppLauncher::with_system_font_family`] wants,
/// the weight set it registers, and the registry and error
/// [`AppLauncher::with_fonts_from`] hands out.
pub use cranpose_render_common::font_source::{
    ANDROID_SYSTEM_FONT_DIR, DEFAULT_SYSTEM_FAMILY_WEIGHTS, FontLoadError, SoftwareTextFontRegistry,
};
pub use host_environment::{host_density, system_font_directory};
pub use native_window::{
    Window, WindowAttachPolicy, WindowConfig, WindowGroup, WindowId, WindowModifierExt,
    WindowMoveMode, WindowNode, WindowResizeDirection, WindowState,
    current_native_window_surface_origin, rememberWindowState,
};
macro_rules! renderer_wgpu_platform_modules {
    ($($name:ident),+ $(,)?) => {
        $(
            #[cfg(all(
                feature = "renderer-wgpu",
                any(
                    feature = "desktop-shell",
                    all(feature = "android", target_os = "android"),
                    all(feature = "ios", target_os = "ios"),
                    all(feature = "web", target_arch = "wasm32")
                )
            ))]
            mod $name;
        )+
    };
}

renderer_wgpu_platform_modules!(present_mode, surface_format, wgpu_surface);

/// The real-time audio engine that backs `cranpose_services::audio`. Call
/// [`install_audio`] once at startup; Android installs it automatically.
#[cfg(feature = "audio")]
pub use cranpose_audio::{AudioEngine, install as install_audio};
/// Core runtime helpers commonly used by applications.
pub use cranpose_core::{
    CoroutineScope, MutableState, SnapshotStateList, SnapshotStateMap, State, delay, interval,
    launchBlocking, mutableStateList, mutableStateListOf, mutableStateMap, mutableStateMapOf,
    mutableStateOf, produceState, remember, rememberCoroutineScope, rememberMutableStateOf,
    rememberMutableStateOfNeverEqual, rememberUpdatedState,
};
/// Liquid UI — the first-party glass component library
/// (`use cranpose::liquid::prelude::*;`).
pub use cranpose_liquid as liquid;
/// The in-process media backend that backs `cranpose_services::media`.
/// Installed automatically by the desktop shell and, wrapped in the platform
/// media session, by the Android one; iOS and the web install their own
/// platform backend instead. [`uri_for_path`] builds the `file:` URI a
/// [`cranpose_services::MediaItem`] takes from a path.
#[cfg(feature = "media")]
pub use cranpose_media::{SoftwareMediaPlayer, path_from_uri, uri_for_path};
/// Re-export framework services (HTTP, URI, etc.) from the dedicated services crate.
pub use cranpose_services::*;
/// Re-export the UI crate so applications can depend on a single crate.
pub use cranpose_ui::*;

static KEEP_SCREEN_ON_EFFECTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Keeps the platform display awake while this call remains in composition and
/// `enabled` is true. Multiple active callers are reference-counted.
#[allow(non_snake_case)]
#[track_caller]
pub fn KeepScreenOn(enabled: bool) {
    cranpose_core::__disposable_effect_impl(
        cranpose_core::caller_location_key()
            ^ cranpose_core::location_key(file!(), line!(), column!()),
        enabled,
        move |scope| {
            if !enabled {
                return cranpose_core::DisposableEffectResult::default();
            }
            if KEEP_SCREEN_ON_EFFECTS.fetch_add(1, std::sync::atomic::Ordering::AcqRel) == 0 {
                cranpose_services::set_keep_screen_on(true);
            }
            scope.on_dispose(move || {
                if KEEP_SCREEN_ON_EFFECTS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) == 1 {
                    cranpose_services::set_keep_screen_on(false);
                }
            })
        },
    );
}

/// Installs a declared bundled-asset set on a worker and returns the outcome
/// on the UI runtime. Work is cancelled with the owning composition.
#[cfg(not(target_arch = "wasm32"))]
#[allow(non_snake_case)]
#[track_caller]
pub fn BundledAssetInstallEffect<K: PartialEq + 'static>(
    keys: K,
    spec: cranpose_services::BundledAssetInstallSpec,
    on_result: impl FnOnce(
        Result<cranpose_services::BundledAssetInstallOutcome, cranpose_services::BundledAssetError>,
    ) + 'static,
) {
    cranpose_core::__launched_effect_impl(
        cranpose_core::caller_location_key()
            ^ cranpose_core::location_key(file!(), line!(), column!()),
        keys,
        move |scope| {
            scope.launch_background(
                move |_token| async move { cranpose_services::install_bundled_asset_set(&spec) },
                on_result,
            );
        },
    );
}

/// Registers a lifecycle observer for the lifetime of the current composition.
///
/// Screens that only need the current state read
/// [`cranpose_services::local_lifecycle_state`] instead; this is for work that
/// must react to a *transition*.
#[allow(non_snake_case)]
#[track_caller]
pub fn LifecycleEffect<K: PartialEq + 'static>(
    keys: K,
    observer: impl FnMut(cranpose_services::LifecycleEvent) + 'static,
) {
    let transitions = cranpose_services::rememberLifecycleEvents();
    cranpose_core::CollectEvents(transitions, keys, observer);
}

static ACTIVE_BACK_HANDLERS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Handles platform back requests on the UI thread while `enabled` is true.
/// Nested handlers follow stack order: the innermost active handler receives
/// the request and dropping it restores the handler beneath it.
#[allow(non_snake_case)]
#[track_caller]
pub fn BackHandler(enabled: bool, mut on_back: impl FnMut() + 'static) {
    let requests = cranpose_core::rememberEventStream(enabled, move |sender| {
        if !enabled {
            return None;
        }
        if ACTIVE_BACK_HANDLERS.fetch_add(1, std::sync::atomic::Ordering::AcqRel) == 0 {
            cranpose_services::set_back_interception(true);
        }
        let registration = cranpose_services::observe_back_requests(move || {
            let count = cranpose_services::take_back_requests();
            if count > 0 {
                sender.send(count);
            }
        });
        Some(BackInterception {
            _registration: registration,
        })
    });
    if enabled {
        cranpose_core::CollectEvents(requests, enabled, move |count: usize| {
            for _ in 0..count {
                on_back();
            }
        });
    }
}

struct BackInterception {
    _registration: cranpose_services::BackRequestObserver,
}

impl Drop for BackInterception {
    fn drop(&mut self) {
        if ACTIVE_BACK_HANDLERS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) == 1 {
            cranpose_services::set_back_interception(false);
        }
    }
}

/// Remembers observable application update state for the current composition.
#[allow(non_snake_case)]
#[track_caller]
pub fn rememberAppUpdateState() -> cranpose_core::State<cranpose_services::AppUpdateStatus> {
    let updates = cranpose_core::rememberEventStream((), |sender| {
        cranpose_services::observe_app_update_status(move |status| sender.send(status))
    });
    cranpose_core::collectAsState(updates, (), cranpose_services::app_update_status())
}

/// Runs `on_frame` on every animation frame while `running` is true.
///
/// This is the framework-owned animation loop for games and other custom-drawn
/// content. `running` is ordinary observable state the caller derives — a
/// simulation flag, "the window is visible", "a gesture is in progress" — and
/// the loop starts and stops with it. There is no wake handle to hold and no
/// scheduler to poke: stopping is a state change like any other.
#[allow(non_snake_case)]
#[track_caller]
pub fn FrameEffect<K: PartialEq + 'static>(
    keys: K,
    running: bool,
    on_frame: impl FnMut(u64) + 'static,
) {
    let on_frame: std::rc::Rc<std::cell::RefCell<dyn FnMut(u64)>> =
        std::rc::Rc::new(std::cell::RefCell::new(on_frame));
    let on_frame = cranpose_core::rememberUpdatedState(on_frame);
    cranpose_core::__launched_effect_async_impl(
        cranpose_core::caller_location_key()
            ^ cranpose_core::location_key(file!(), line!(), column!()),
        std::panic::Location::caller().into(),
        (keys, running),
        move |scope| {
            Box::pin(async move {
                if !running {
                    return;
                }
                let clock = scope.runtime().frame_clock();
                while scope.is_active() {
                    let now = clock.next_frame().await;
                    if !scope.is_active() {
                        break;
                    }
                    (on_frame.value().borrow_mut())(now);
                }
            })
        },
    );
}

#[doc(hidden)]
pub use cranpose_core::{
    __branch_group_scope_deferred, CallbackHolder, Composer, Key, ParamState, ReturnSlot,
    branch_location_key, cached_branch_location_key, cached_composable_definition_key,
    caller_location_key, composable_definition_key, composable_identity_key,
    debug_label_current_scope, location_key, with_current_composer,
};

#[cfg(all(
    feature = "desktop-shell",
    feature = "robot",
    feature = "renderer-wgpu"
))]
#[doc(hidden)]
pub type RobotAppHook = dyn FnMut(String, String) -> Result<Option<String>, String>;

/// Guides for using Cranpose, compiled with the crate so they cannot drift from
/// it. Built only for documentation, so they cost a reader nothing at runtime.
#[cfg(doc)]
pub mod _docs;

/// Convenience imports for Cranpose applications.
pub mod prelude {
    pub use cranpose_core::{
        CoroutineScope, MutableState, SnapshotStateList, SnapshotStateMap, State, delay, interval,
        launchBlocking, mutableStateList, mutableStateListOf, mutableStateMap, mutableStateMapOf,
        mutableStateOf, produceState, remember, rememberCoroutineScope, rememberMutableStateOf,
        rememberMutableStateOfNeverEqual, rememberUpdatedState,
    };
    pub use cranpose_services::*;
    pub use cranpose_ui::*;

    #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
    pub use crate::{
        AndroidHostWindowPositionError, AndroidHostWindowSizeError, AndroidHostWindowSizeStatus,
        AndroidHostWindowState, rememberAndroidHostWindowState,
    };
    pub use crate::{
        AndroidOverlayWindowOptions, AppLauncher, AppSettings, Window, WindowAttachPolicy,
        WindowConfig, WindowGroup, WindowId, WindowModifierExt, WindowMoveMode, WindowNode,
        WindowResizeDirection, WindowState, rememberWindowState,
    };
}

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub(crate) mod platform_env;

#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
pub mod android;
#[cfg(feature = "renderer-wgpu")]
#[cfg_attr(
    not(any(
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios")
    )),
    allow(dead_code)
)]
pub(crate) mod gpu_limits;

#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
pub mod desktop;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod desktop_accessibility;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod desktop_bundled_assets;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod desktop_host_surface;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod desktop_incoming;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod desktop_input;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod desktop_power;
#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
mod host_surface_resize;

#[cfg(all(
    unix,
    feature = "renderer-wgpu",
    any(
        feature = "desktop-shell",
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios")
    )
))]
mod process_info;

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
mod winit_pointer;

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
#[cfg_attr(not(all(feature = "ios", target_os = "ios")), allow(dead_code))]
mod winit_touch;

#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod winit_wheel;

#[cfg(all(
    feature = "robot",
    feature = "desktop-shell",
    feature = "renderer-wgpu"
))]
mod robot;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
pub mod ios;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_accessibility;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_pick_future;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_file_picker;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_uri_handler;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_clipboard;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_share_sheet;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_image_picker;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_notifier;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_haptics;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_media;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_app_info;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_device_info;

#[cfg(any(
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        target_os = "macos"
    )
))]
mod apple_thermal;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_writable_folder;

#[cfg(any(
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(
        feature = "camera-desktop",
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        target_os = "macos"
    )
))]
mod apple_camera;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_keyboard;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_back_gesture;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_background;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_bundled_assets;

#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
pub mod recorder;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
pub mod web;

#[cfg(any(
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"),
    test
))]
mod web_surface_scale;

#[cfg(any(
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"),
    test
))]
mod web_wheel;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
mod web_accessibility;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
mod web_clipboard;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
mod web_host_surface;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
mod web_media;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
mod web_services;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
mod web_power;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
mod web_drop;

/// Development frame pacing and FPS statistics types.
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
pub use cranpose_app_shell::{DevOptions, FpsStats, FramePacingMode};
#[cfg(all(
    feature = "desktop-shell",
    feature = "robot",
    feature = "renderer-wgpu"
))]
pub use robot::{
    Robot, RobotScreenshot, RobotTimelineAction, RobotTimelineStep, SemanticElement, SemanticRect,
};

#[cfg(all(test, feature = "desktop-shell", feature = "renderer-wgpu"))]
pub(crate) fn test_scratch_dir(tag: &str) -> std::path::PathBuf {
    cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), tag)
}
