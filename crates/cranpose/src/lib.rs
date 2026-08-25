#![deny(missing_docs)]

//! High level utilities for running Cranpose applications with minimal boilerplate.

#[cfg(all(feature = "android", target_os = "android"))]
mod android_file_picker;
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
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_accessibility;
/// The Android accessibility wire format behind `CranposeActivity`'s
/// `AccessibilityNodeProvider`. Built on the host as well so its encoding
/// tests run everywhere.
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
mod android_accessibility_wire;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_app_info;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_camera;
#[cfg(all(feature = "android", feature = "media", target_os = "android"))]
mod android_media;
// `android_main!` expands to nothing off Android, so the macro itself is always
// compiled: an application writes the invocation once and every target accepts
// it.
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
/// The bounded command queue behind the Android haptics delivery thread.
/// Built on the host as well so its ordering/coalescing test runs everywhere.
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
/// The Android intent-extra wire format behind `cranpose_services::launch_args`.
/// Built on the host as well so its decoding tests run everywhere.
#[cfg(any(test, all(feature = "android", target_os = "android")))]
mod android_launch_args;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_overlay_window;
/// Panic-hook chaining behind `android.rs`'s `run()`. Built on the host as
/// well so the chaining logic's test runs everywhere.
#[cfg(any(test, all(feature = "android", target_os = "android")))]
mod android_panic_hook;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_perf_hint;
/// The Play Billing wire format behind `cranpose_services::purchases`. Built on
/// the host as well so its decoding tests run everywhere.
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
    rememberAndroidHostWindowState, AndroidHostWindowPositionError, AndroidHostWindowSizeError,
    AndroidHostWindowSizeStatus, AndroidHostWindowState,
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
    FontLoadError, SoftwareTextFontRegistry, ANDROID_SYSTEM_FONT_DIR, DEFAULT_SYSTEM_FAMILY_WEIGHTS,
};
pub use host_environment::{host_density, system_font_directory};
pub use native_window::{
    current_native_window_surface_origin, rememberWindowState, Window, WindowAttachPolicy,
    WindowConfig, WindowGroup, WindowId, WindowModifierExt, WindowMoveMode, WindowNode,
    WindowResizeDirection, WindowState,
};
#[cfg(all(
    feature = "renderer-wgpu",
    any(
        feature = "desktop-shell",
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios"),
        all(feature = "web", target_arch = "wasm32")
    )
))]
mod present_mode;
#[cfg(all(
    feature = "renderer-wgpu",
    any(
        feature = "desktop-shell",
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios"),
        all(feature = "web", target_arch = "wasm32")
    )
))]
mod surface_format;
#[cfg(all(
    feature = "renderer-wgpu",
    any(
        feature = "desktop-shell",
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios"),
        all(feature = "web", target_arch = "wasm32")
    )
))]
mod wgpu_surface;

/// The real-time audio engine that backs `cranpose_services::audio`. Call
/// [`install_audio`] once at startup; Android installs it automatically.
#[cfg(feature = "audio")]
pub use cranpose_audio::{install as install_audio, AudioEngine};
/// Core runtime helpers commonly used by applications.
pub use cranpose_core::{
    delay, interval, launchBlocking, mutableStateOf, produceState, remember,
    rememberCoroutineScope, rememberMutableStateOf, rememberMutableStateOfNeverEqual,
    rememberUpdatedState, CoroutineScope, MutableState, SnapshotStateList, SnapshotStateMap, State,
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
pub use cranpose_media::{path_from_uri, uri_for_path, SoftwareMediaPlayer};
/// Re-export framework services (HTTP, URI, etc.) from the dedicated services crate.
pub use cranpose_services::*;
/// Re-export the UI crate so applications can depend on a single crate.
pub use cranpose_ui::*;

static KEEP_SCREEN_ON_EFFECTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Keeps the platform display awake while this call remains in composition and
/// `enabled` is true. Multiple active callers are reference-counted.
#[allow(non_snake_case)]
pub fn KeepScreenOn(enabled: bool) {
    cranpose_core::DisposableEffect!(enabled, move |scope| {
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
    });
}

/// Installs a declared bundled-asset set on a worker and returns the outcome
/// on the UI runtime. Work is cancelled with the owning composition.
#[cfg(not(target_arch = "wasm32"))]
#[allow(non_snake_case)]
pub fn BundledAssetInstallEffect<K: PartialEq + 'static>(
    keys: K,
    spec: cranpose_services::BundledAssetInstallSpec,
    on_result: impl FnOnce(
            Result<
                cranpose_services::BundledAssetInstallOutcome,
                cranpose_services::BundledAssetError,
            >,
        ) + 'static,
) {
    cranpose_core::LaunchedEffect!(keys, move |scope| {
        scope.launch_background(
            move |_token| async move { cranpose_services::install_bundled_asset_set(&spec) },
            on_result,
        );
    });
}

/// Registers a lifecycle observer for the lifetime of the current composition.
///
/// Screens that only need the current state read
/// [`cranpose_services::local_lifecycle_state`] instead; this is for work that
/// must react to a *transition*.
#[allow(non_snake_case)]
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
    // Only a handler that is switched on collects. A collector is a runtime
    // task parked on a stream for as long as it is composed, and the shell
    // composes a `BackHandler` at the root of every application — so collecting
    // while disabled meant every Cranpose app carried one parked task for its
    // whole life, for a handler that had nothing to receive.
    if enabled {
        cranpose_core::CollectEvents(requests, enabled, move |count: usize| {
            for _ in 0..count {
                on_back();
            }
        });
    }
}

/// Holds the platform back registration and releases the interception flag when
/// the last handler leaves the composition.
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
pub fn FrameEffect<K: PartialEq + 'static>(
    keys: K,
    running: bool,
    on_frame: impl FnMut(u64) + 'static,
) {
    let on_frame: std::rc::Rc<std::cell::RefCell<dyn FnMut(u64)>> =
        std::rc::Rc::new(std::cell::RefCell::new(on_frame));
    let on_frame = cranpose_core::rememberUpdatedState(on_frame);
    cranpose_core::LaunchedEffectAsync!((keys, running), move |scope| {
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
    });
}

#[doc(hidden)]
pub use cranpose_core::{
    __branch_group_scope, branch_location_key, debug_label_current_scope, location_key,
    with_current_composer, CallbackHolder, Composer, ParamState, ReturnSlot,
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
        delay, interval, mutableStateOf, produceState, remember, rememberCoroutineScope,
        rememberMutableStateOf, rememberMutableStateOfNeverEqual, rememberUpdatedState,
        CoroutineScope, MutableState, SnapshotStateList, SnapshotStateMap, State,
    };
    pub use cranpose_services::*;
    pub use cranpose_ui::*;

    #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
    pub use crate::{
        rememberAndroidHostWindowState, AndroidHostWindowPositionError, AndroidHostWindowSizeError,
        AndroidHostWindowSizeStatus, AndroidHostWindowState,
    };
    pub use crate::{
        rememberWindowState, AndroidOverlayWindowOptions, AppLauncher, AppSettings, Window,
        WindowAttachPolicy, WindowConfig, WindowGroup, WindowId, WindowModifierExt, WindowMoveMode,
        WindowNode, WindowResizeDirection, WindowState,
    };
}

// Platform-specific runtime modules
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

/// What this process is using: the memory and processor-time readings whose
/// platform calls need `libc`, so no application writes them. Compiled with
/// the platform shells that install it — a target with no shell has nothing to
/// install it into.
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

/// Compiled with the shells that translate winit input. Both need a renderer:
/// a shell with nothing to draw with never opens a window to receive a pointer
/// event, so building this without one leaves it with no callers at all.
#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
mod winit_pointer;

/// Multi-touch id routing for the winit ingress. Only the iOS shell consumes it
/// today (desktop pointers are single-finger), but it is built on every target
/// that compiles the winit translation so its tests run on the host.
#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
#[cfg_attr(not(all(feature = "ios", target_os = "ios")), allow(dead_code))]
mod winit_touch;

/// winit's mouse wheel, normalized into the shell's shared wheel sample.
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod winit_wheel;

/// Renderer-agnostic robot testing harness shared by the desktop shells.
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

/// Thermal pressure, which both Apple shells read from the same Foundation call.
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

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_camera;

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

// The canvas drawing-buffer sizing policy is pure arithmetic shared with the
// web runtime. Compile it for the wasm web build (where `web` consumes it) and
// under `test` so its HiDPI regression guards run in the host test suite.
#[cfg(any(
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"),
    test
))]
mod web_surface_scale;

// The browser's wheel units and sign convention are likewise pure arithmetic,
// and likewise silent when wrong: compile them under `test` so the host suite
// pins the direction the web scrolls.
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

// Re-export the renderer-agnostic robot harness so applications and the
// testing crate can drive either desktop shell through a single path.
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

/// A unique, empty directory under the workspace `target/test-output` for a
/// test in this crate that needs real files. See
/// [`cranpose_core::test_scratch_dir`].
///
/// Gated the way its callers are: the tests that need real files are the
/// desktop shell's own, and an iOS or web build compiles neither them nor this.
#[cfg(all(test, feature = "desktop-shell", feature = "renderer-wgpu"))]
pub(crate) fn test_scratch_dir(tag: &str) -> std::path::PathBuf {
    cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), tag)
}
