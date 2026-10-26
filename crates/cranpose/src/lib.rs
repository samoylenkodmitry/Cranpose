#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

extern crate self as cranpose;

#[cfg(feature = "image-loading")]
mod async_image;
#[cfg(feature = "localization")]
mod system_languages;
#[cfg(feature = "localization")]
pub use system_languages::{
    ProvideApplicationLocalization, local_system_languages, system_languages,
};

#[cfg(any(test, all(feature = "android", target_os = "android")))]
pub mod android;
/// An in-process Cranpose component driven by its native host.
#[cfg(all(feature = "renderer-wgpu", not(target_arch = "wasm32")))]
pub mod embedded_view;
#[cfg(all(feature = "renderer-wgpu", not(target_arch = "wasm32")))]
pub use cranpose_app_shell::TouchRouter;
#[cfg(feature = "renderer-wgpu")]
mod external_frames;
#[cfg(feature = "renderer-wgpu")]
pub use external_frames::{ExternalFrames, FrameTexture, FrameWriter};
#[cfg(feature = "renderer-wgpu")]
mod gpu_context;
#[cfg(feature = "renderer-wgpu")]
pub use gpu_context::{GpuContext, GpuRequirements, gpu_context};
#[cfg(all(
    feature = "renderer-wgpu",
    any(
        feature = "desktop-shell",
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios"),
        all(feature = "web", target_arch = "wasm32")
    )
))]
mod gpu_loss;
#[cfg(feature = "renderer-wgpu")]
mod guest_view;
#[cfg(feature = "renderer-wgpu")]
pub use guest_view::{GuestView, GuestViewError};
#[cfg(all(feature = "renderer-wgpu", not(target_arch = "wasm32")))]
mod frame_readback;
/// Native views mounted in a Cranpose layout by an application-owned host.
pub mod native_view;
#[cfg(all(feature = "renderer-wgpu", not(target_arch = "wasm32")))]
mod offscreen_instance;
mod scoped_weak_stack;
#[cfg(all(feature = "android", target_os = "android"))]
pub use android::file_picker::open_content_uri;
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
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
mod accessibility_publish_policy;
mod android_entry;
#[cfg_attr(
    not(all(feature = "android", target_os = "android")),
    expect(dead_code, reason = "only the Android backend hosts windows")
)]
#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    expect(
        unreachable_pub,
        reason = "only an Android build with the wgpu renderer exports the host window"
    )
)]
mod android_host_window;
mod android_input;
#[cfg(target_os = "android")]
mod android_symbols;
mod app_launcher;
#[cfg(feature = "framebuffer")]
pub mod framebuffer;
#[cfg(feature = "preview")]
pub mod preview;
#[cfg(feature = "watchos")]
pub mod watchos;
#[cfg(feature = "preview")]
pub use cranpose_macros::preview;
#[cfg(feature = "embed")]
pub mod inspection;
#[cfg(feature = "renderer-wgpu")]
pub use cranpose_app_shell::inspector::{
    InspectorAction, InspectorControl, InspectorMode, InspectorNode, InspectorState,
};
mod host_environment;
#[cfg(all(feature = "ios", target_os = "ios"))]
pub mod ios;
mod native_window;
mod thread_state;
mod window_local;
#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
mod window_node;
/// Android activity state passed to the [`android_main!`] entry point.
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
pub use app_launcher::{
    AndroidGpuBackend, AndroidOverlayWindowOptions, AppFonts, AppLauncher, AppSettings,
    CustomCursorSize, DefaultFont, LauncherFonts,
};
#[cfg(feature = "localized-arabic")]
pub use cranpose_fonts::ARABIC as ARABIC_FONT_PACK;
#[cfg(feature = "localized-cjk")]
pub use cranpose_fonts::CJK as CJK_FONT_PACK;
#[cfg(feature = "localized-devanagari")]
pub use cranpose_fonts::DEVANAGARI as DEVANAGARI_FONT_PACK;
#[cfg(any(
    feature = "localized-arabic",
    feature = "localized-devanagari",
    feature = "localized-cjk"
))]
pub use cranpose_fonts::FontPack;
pub use cranpose_render_common::font_source::{
    ANDROID_SYSTEM_FONT_DIR, DEFAULT_SYSTEM_FAMILY_WEIGHTS, FontLoadError, SoftwareTextFontRegistry,
};
#[cfg(feature = "installed-fonts")]
pub use cranpose_render_common::installed_fonts::{install_font_bytes, install_font_file};
/// Shared font data accepted by [`AppLauncher::with_font_face_bytes`].
pub use cranpose_render_common::software_text_raster::FontBytes;
pub use host_environment::{host_density, system_font_directory};
/// The JNI types the [`android_main!`] exports name.
#[cfg(all(feature = "android", target_os = "android"))]
#[doc(hidden)]
pub use jni as __android_jni;
pub use native_window::{
    WindowConfig, WindowFocus, WindowModifierExt, WindowResizeDirection, WindowState,
    rememberWindowState, rememberWindowStateAt,
};
pub use window_local::LocalWindowState;
/// Includes the app capability declaration from the build script.
///
/// Defines `pub const CAPABILITIES: cranpose::capabilities::Capabilities` from
/// the file produced by `cranpose::capabilities::Declaration::emit`:
///
/// ```ignore
/// cranpose::app_capabilities!();
///
/// cranpose::android_main! {
///     launcher: cranpose::AppLauncher::new().with_capabilities(&CAPABILITIES),
///     content: my_app::Root,
/// }
/// ```
#[macro_export]
macro_rules! app_capabilities {
    () => {
        #[doc(hidden)]
        mod cranpose_declared_capabilities {
            use $crate::capabilities as cranpose_capabilities;

            include!(concat!(env!("OUT_DIR"), "/cranpose_capabilities.rs"));
        }
        pub use cranpose_declared_capabilities::CAPABILITIES;
    };
}

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

#[cfg(feature = "audio")]
pub use cranpose_audio::{AudioEngine, install as install_audio};
/// Declare app services, permissions and required hardware in the build script.
/// [`app_capabilities!`](crate::app_capabilities) includes the generated values.
pub use cranpose_capabilities as capabilities;
mod composition_api {
    pub use cranpose_core::{
        BlockingError, BlockingExecutor, BlockingExecutorConfig, BlockingTask, CollectEvents,
        ComputedState, CoroutineScope, DisposableEffect, DisposableEffectResult,
        DisposableEffectScope, EventStream, LaunchedEffect, LaunchedEffectAsync,
        LaunchedEffectScope, MovableContent, MutableState, SnapshotStateList, SnapshotStateMap,
        State, TaskHandle, collectAsState, computedStateOf, delay, derivedStateOf, forget_movable,
        interval, key, launchBlocking, movable, movableContentOf, mutableStateList,
        mutableStateListOf, mutableStateMap, mutableStateMapOf, mutableStateOf,
        mutableStateOfNeverEqual, produceState, remember, rememberCoroutineScope,
        rememberEventStream, rememberKeyed, rememberMovableContentOf, rememberMutableStateOf,
        rememberMutableStateOfNeverEqual, rememberUpdatedState, withBlocking,
    };
}
#[cfg(feature = "image-loading")]
pub use async_image::AsyncImage;
pub use composition_api::*;
/// Glass controls, themes and motion for Cranpose apps.
/// Import common components with `use cranpose::liquid::prelude::*;`.
pub use cranpose_liquid as liquid;
#[cfg(feature = "media-backend")]
pub use cranpose_media::{SoftwareMediaPlayer, path_from_uri, uri_for_path};
/// Whether the host's current application theme is dark.
pub use cranpose_services::isSystemInDarkTheme;
pub use cranpose_services::*;
pub use cranpose_ui::*;

static KEEP_SCREEN_ON_EFFECTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Keeps the platform display awake while this call remains in composition and
/// `enabled` is true. Multiple active callers are reference-counted.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn KeepScreenOn(enabled: bool) {
    cranpose_core::__disposable_effect_impl(
        caller_location_key() ^ cranpose_core::location_key!(),
        enabled,
        move |scope| {
            if !enabled {
                return DisposableEffectResult::default();
            }
            if KEEP_SCREEN_ON_EFFECTS.fetch_add(1, std::sync::atomic::Ordering::AcqRel) == 0 {
                set_keep_screen_on(true);
            }
            scope.on_dispose(move || {
                if KEEP_SCREEN_ON_EFFECTS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) == 1 {
                    set_keep_screen_on(false);
                }
            })
        },
    );
}

/// Installs a declared bundled-asset set on a worker and returns the outcome
/// on the UI runtime. Work is cancelled with the owning composition.
#[cfg(not(target_arch = "wasm32"))]
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn BundledAssetInstallEffect<K: PartialEq + 'static>(
    keys: K,
    spec: BundledAssetInstallSpec,
    on_result: impl FnOnce(Result<BundledAssetInstallOutcome, BundledAssetError>) + 'static,
) {
    cranpose_core::__launched_effect_impl(
        caller_location_key() ^ cranpose_core::location_key!(),
        keys,
        move |scope| {
            scope.launch_background(
                move |_token| async move { install_bundled_asset_set(&spec) },
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
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn LifecycleEffect<K: PartialEq + 'static>(
    keys: K,
    observer: impl FnMut(LifecycleEvent) + 'static,
) {
    let transitions = rememberLifecycleEvents();
    CollectEvents(transitions, keys, observer);
}

/// Remembers observable application update state for the current composition.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberAppUpdateState() -> State<AppUpdateStatus> {
    let updates = rememberEventStream((), |sender| {
        observe_app_update_status(move |status| sender.send(status))
    });
    collectAsState(updates, (), app_update_status())
}

/// Runs `on_frame` on every animation frame while `running` is true.
///
/// This is the framework-owned animation loop for games and other custom-drawn
/// content. `running` is ordinary observable state the caller derives — a
/// simulation flag, "the window is visible", "a gesture is in progress" — and
/// the loop starts and stops with it. There is no wake handle to hold and no
/// scheduler to poke: stopping is a state change like any other.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn FrameEffect<K: PartialEq + 'static>(
    keys: K,
    running: bool,
    on_frame: impl FnMut(u64) + 'static,
) {
    let on_frame: std::rc::Rc<std::cell::RefCell<dyn FnMut(u64)>> =
        std::rc::Rc::new(std::cell::RefCell::new(on_frame));
    let on_frame = rememberUpdatedState(on_frame);
    cranpose_core::__launched_effect_async_impl(
        caller_location_key() ^ cranpose_core::location_key!(),
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
    __branch_group_scope_advance, __branch_group_scope_deferred, __constant, __current_composer,
    __source_scope, CallbackHolder, ComposableGroup, Composer, Key, ParamReturnState, ParamState,
    RecomposeScope, ReturnSlot, ValueSlotHandle, branch_location_key,
    cached_composable_definition_key, caller_location_key, composable_definition_key,
    composable_identity_key, const_location_key, constant_value_key, debug_label_current_scope,
    hot_branch_key, hot_definition_key, hot_origin, location_key, noted_location_key,
    refresh_param, refresh_shared_param, store_param, with_current_composer,
};

#[cfg(all(
    feature = "desktop-shell",
    feature = "robot",
    feature = "renderer-wgpu"
))]
#[doc(hidden)]
pub type RobotAppHook = dyn FnMut(String, String) -> Result<Option<String>, String>;

/// App setup, state, layout and platform guides.
#[cfg(doc)]
pub mod _docs;

/// Convenience imports for Cranpose applications.
pub mod prelude {
    pub use cranpose_services::*;
    pub use cranpose_ui::*;

    #[cfg(feature = "image-loading")]
    pub use crate::async_image::AsyncImage;
    #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
    pub use crate::{
        AndroidHostWindowPositionError, AndroidHostWindowSizeError, AndroidHostWindowSizeStatus,
        AndroidHostWindowState, rememberAndroidHostWindowState,
    };
    pub use crate::{
        AndroidOverlayWindowOptions, AppFonts, AppLauncher, AppSettings, WebView, WebViewEvent,
        WindowConfig, WindowModifierExt, WindowResizeDirection, WindowState, composition_api::*,
        rememberWindowState, rememberWindowStateAt,
    };
    #[cfg(all(
        feature = "desktop-menus",
        feature = "renderer-wgpu",
        any(
            target_os = "macos",
            target_os = "windows",
            all(target_os = "linux", not(target_arch = "wasm32"))
        )
    ))]
    pub use crate::{CheckItem, KeyShortcut, Menu, MenuBar, MenuEntry, MenuItem, Tray, TraySpec};
}

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"),
    feature = "embed",
    feature = "framebuffer"
))]
pub(crate) mod platform_env;

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    all(feature = "android", target_os = "android"),
    all(feature = "ios", target_os = "ios"),
    feature = "embed"
))]
mod pipeline_cache_file;

#[cfg(any(
    all(feature = "desktop-shell", feature = "renderer-wgpu"),
    feature = "embed"
))]
mod application_id;

#[cfg(feature = "embed")]
pub mod embed;
#[cfg(feature = "embed")]
mod embed_frame;
#[cfg(feature = "embed")]
mod embed_input;
#[cfg(feature = "embed")]
mod embed_overlay;
#[cfg(feature = "embed")]
mod embed_protocol;
#[cfg(feature = "embed")]
mod embed_surfaces;

#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
mod frame_lead;
#[cfg(any(
    test,
    all(feature = "android", feature = "renderer-wgpu", target_os = "android")
))]
mod frame_pacer;
#[cfg(feature = "renderer-wgpu")]
#[cfg_attr(
    not(any(
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios")
    )),
    allow(
        dead_code,
        reason = "only the Android and iOS backends set memory hints"
    )
)]
pub(crate) mod gpu_limits;
#[cfg(any(test, all(feature = "android", target_os = "android")))]
mod vsync_period;

#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod cursor_scale;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
pub mod desktop;
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
pub use desktop::integration::{
    DesktopIntegration, DesktopIntegrationContext, DesktopWaker, DesktopWindow, DesktopWindowId,
};
#[cfg(all(
    feature = "desktop-menus",
    feature = "renderer-wgpu",
    any(
        target_os = "macos",
        target_os = "windows",
        all(target_os = "linux", not(target_arch = "wasm32"))
    )
))]
pub use desktop::menus::{
    CheckItem, KeyShortcut, Menu, MenuBar, MenuEntry, MenuItem, Tray, TraySpec, WindowMenuBar,
};
/// The raw window handle traits a [`DesktopWindow`] implements, at the
/// version the shell uses.
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
pub use raw_window_handle;
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

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod winit_touch;

#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
mod winit_wheel;

#[cfg(all(
    feature = "robot",
    feature = "desktop-shell",
    feature = "renderer-wgpu"
))]
mod robot;

#[cfg(target_vendor = "apple")]
mod apple;

#[cfg(all(feature = "watchos", target_os = "watchos"))]
mod watchos_services;

#[cfg(all(feature = "watchos", feature = "heart-rate", target_os = "watchos"))]
mod watchos_heart_rate;

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    target_os = "linux"
))]
mod linux_network;

#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
#[expect(
    clippy::print_stderr,
    reason = "the recorder reports to the developer who started it"
)]
pub mod recorder;

#[cfg(any(
    test,
    all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
))]
pub mod web;

/// Development frame pacing and FPS statistics types.
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
pub use cranpose_app_shell::{DevOptions, FpsStats, FramePacingMode};
#[cfg(all(
    feature = "desktop-shell",
    feature = "robot",
    feature = "renderer-wgpu"
))]
pub use cranpose_render_wgpu::{pipelines_created, pipelines_created_off_frame};
#[cfg(all(
    feature = "desktop-shell",
    feature = "robot",
    feature = "renderer-wgpu"
))]
pub use robot::{
    Robot, RobotPresentationInfo, RobotScreenshot, RobotTimelineAction, RobotTimelineStep,
    SemanticElement, SemanticRect,
};

#[cfg(all(test, feature = "desktop-shell", feature = "renderer-wgpu"))]
pub(crate) fn test_scratch_dir(tag: &str) -> std::path::PathBuf {
    cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), tag)
}
mod webview;
pub use webview::{WebView, WebViewEvent};

#[cfg(all(
    feature = "webview",
    feature = "renderer-wgpu",
    any(
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios"),
        all(feature = "web", target_arch = "wasm32"),
        all(
            feature = "desktop-shell",
            any(target_os = "macos", target_os = "windows", target_os = "linux")
        )
    )
))]
mod webview_host;
#[cfg(all(
    feature = "webview",
    feature = "renderer-wgpu",
    any(
        all(feature = "ios", target_os = "ios"),
        all(
            feature = "desktop-shell",
            any(target_os = "macos", target_os = "windows", target_os = "linux")
        )
    )
))]
mod wry_webview;
