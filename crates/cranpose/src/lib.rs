#![deny(missing_docs)]

//! High level utilities for running Cranpose applications with minimal boilerplate.

#[cfg(not(any(
    feature = "desktop",
    feature = "android",
    feature = "web",
    feature = "ios"
)))]
compile_error!(
    "cranpose must be built with at least one of `desktop`, `android`, `web`, or `ios` features."
);

#[cfg(not(any(feature = "renderer-pixels", feature = "renderer-wgpu")))]
compile_error!("cranpose requires either `renderer-pixels` or `renderer-wgpu` feature.");

#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
mod android_host_window;
#[cfg(all(feature = "android", target_os = "android"))]
mod android_jni;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod android_overlay_window;
mod launcher;
mod native_window;
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
pub use android_host_window::{
    rememberAndroidHostWindowState, AndroidHostWindowPositionError, AndroidHostWindowSizeError,
    AndroidHostWindowSizeStatus, AndroidHostWindowState,
};
#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub use launcher::LaunchError;
pub use launcher::{AndroidOverlayWindowOptions, AppLauncher, AppSettings};
pub use native_window::{
    current_native_window_surface_origin, rememberWindowState, Window, WindowAttachPolicy,
    WindowConfig, WindowGroup, WindowId, WindowModifierExt, WindowMoveMode, WindowNode,
    WindowResizeDirection, WindowState,
};
#[cfg(feature = "renderer-wgpu")]
mod present_mode;
#[cfg(feature = "renderer-wgpu")]
mod wgpu_surface;

/// Re-export framework services (HTTP, URI, etc.) from the dedicated services crate.
pub use cranpose_services::*;
/// Re-export the UI crate so applications can depend on a single crate.
pub use cranpose_ui::*;

/// Core runtime helpers commonly used by applications.
pub use cranpose_core::{mutableStateOf, remember, rememberUpdatedState, useState};

#[doc(hidden)]
pub use cranpose_core::{
    debug_label_current_scope, location_key, with_current_composer, CallbackHolder, Composer,
    ParamState, ReturnSlot,
};

#[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
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
    pub use cranpose_core::{mutableStateOf, remember, rememberUpdatedState, useState};
    pub use cranpose_services::*;
    pub use cranpose_ui::*;
}

// Platform-specific runtime modules
#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
pub mod android;

#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub mod desktop;

#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub mod recorder;

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
pub mod web;

// Re-export Robot type from desktop module when robot feature is enabled
#[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
pub use desktop::{Robot, RobotScreenshot, SemanticElement, SemanticRect};

/// FPS monitoring API - use these to track frame rate for performance optimization.
///
/// - `current_fps()` - Get current FPS value
/// - `fps_stats()` - Get detailed frame statistics (avg ms, recomps/sec)
/// - `fps_display()` - Get formatted FPS string for display
/// - `fps_display_detailed()` - Get detailed stats string
#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub use cranpose_app_shell::{
    current_fps, fps_display, fps_display_detailed, fps_stats, DevOptions, FpsStats,
    FramePacingMode,
};
