#![deny(unsafe_code)]
#![deny(missing_docs)]

//! High level utilities for running Cranpose applications with minimal boilerplate.

#[cfg(all(feature = "android", target_os = "android"))]
mod android_file_picker;
#[cfg(all(feature = "android", target_os = "android"))]
pub use android_file_picker::open_content_uri;
#[cfg(any(
    test,
    feature = "desktop-shell",
    all(feature = "android", target_os = "android"),
    all(feature = "ios", target_os = "ios"),
    all(feature = "web", target_arch = "wasm32")
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
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_display;
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
mod launcher;
mod native_window;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
pub use android_host_window::{
    rememberAndroidHostWindowState, AndroidHostWindowPositionError, AndroidHostWindowSizeError,
    AndroidHostWindowSizeStatus, AndroidHostWindowState,
};
/// Font registration vocabulary named by [`AppLauncher`]'s font methods:
/// the platform font directory [`AppLauncher::with_system_font_family`] wants,
/// the weight set it registers, and the registry and error
/// [`AppLauncher::with_fonts_from`] hands out.
pub use cranpose_render_common::font_source::{
    FontLoadError, SoftwareTextFontRegistry, ANDROID_SYSTEM_FONT_DIR, DEFAULT_SYSTEM_FAMILY_WEIGHTS,
};
#[cfg(all(
    feature = "renderer-wgpu",
    any(feature = "desktop-shell", all(feature = "ios", target_os = "ios"))
))]
pub use launcher::LaunchError;
pub use launcher::{AndroidOverlayWindowOptions, AppLauncher, AppSettings};
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

/// Re-export framework services (HTTP, URI, etc.) from the dedicated services crate.
pub use cranpose_services::*;
/// Re-export the UI crate so applications can depend on a single crate.
pub use cranpose_ui::*;

/// Liquid UI — the first-party glass component library
/// (`use cranpose::liquid::prelude::*;`).
pub use cranpose_liquid as liquid;

/// The real-time audio engine that backs `cranpose_services::audio`. Call
/// [`install_audio`] once at startup; Android installs it automatically.
#[cfg(feature = "audio")]
pub use cranpose_audio::{install as install_audio, AudioEngine};

/// Core runtime helpers commonly used by applications.
pub use cranpose_core::{mutableStateOf, remember, rememberUpdatedState, useState, useStateRaw};

#[doc(hidden)]
pub use cranpose_core::{
    debug_label_current_scope, location_key, with_current_composer, CallbackHolder, Composer,
    ParamState, ReturnSlot,
};

#[cfg(all(
    feature = "desktop-shell",
    feature = "robot",
    feature = "renderer-wgpu"
))]
#[doc(hidden)]
pub type RobotAppHook = dyn FnMut(String, String) -> Result<Option<String>, String>;

/// Convenience imports for Cranpose applications.
pub mod prelude {
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
    pub use cranpose_core::{
        mutableStateOf, remember, rememberUpdatedState, useState, useStateRaw,
    };
    pub use cranpose_services::*;
    pub use cranpose_ui::*;
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
mod desktop_input;

#[cfg(any(feature = "desktop-shell", all(feature = "ios", target_os = "ios")))]
mod winit_pointer;

/// Multi-touch id routing for the winit ingress. Only the iOS shell consumes it
/// today (desktop pointers are single-finger), but it is built on every target
/// that compiles the winit translation so its tests run on the host.
#[cfg(any(feature = "desktop-shell", all(feature = "ios", target_os = "ios")))]
#[cfg_attr(not(all(feature = "ios", target_os = "ios")), allow(dead_code))]
mod winit_touch;

/// winit's mouse wheel, normalized into the shell's shared wheel sample.
#[cfg(feature = "desktop-shell")]
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
mod ios_app_info;

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod ios_device_info;

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
mod web_services;

// Re-export the renderer-agnostic robot harness so applications and the
// testing crate can drive either desktop shell through a single path.
#[cfg(all(
    feature = "desktop-shell",
    feature = "robot",
    feature = "renderer-wgpu"
))]
pub use robot::{
    Robot, RobotScreenshot, RobotTimelineAction, RobotTimelineStep, SemanticElement, SemanticRect,
};

/// Development frame pacing and FPS statistics types.
#[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
pub use cranpose_app_shell::{DevOptions, FpsStats, FramePacingMode};
