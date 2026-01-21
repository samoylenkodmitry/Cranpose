#![deny(missing_docs)]

//! High level utilities for running Cranpose applications with minimal boilerplate.

#[cfg(not(any(feature = "desktop", feature = "android", feature = "web")))]
compile_error!(
    "cranpose must be built with at least one of `desktop`, `android`, or `web` features."
);

#[cfg(not(any(feature = "renderer-pixels", feature = "renderer-wgpu")))]
compile_error!("cranpose requires either `renderer-pixels` or `renderer-wgpu` feature.");

mod launcher;
pub use launcher::{AppLauncher, AppSettings};
#[cfg(feature = "renderer-wgpu")]
mod present_mode;

/// Re-export the UI crate so applications can depend on a single crate.
pub use cranpose_ui::*;

/// Core runtime helpers commonly used by applications.
pub use cranpose_core::{mutableStateOf, remember, rememberUpdatedState, useState};

#[doc(hidden)]
pub use cranpose_core::{
    location_key, with_current_composer, CallbackHolder, Composer, ParamState, ReturnSlot,
};

/// Convenience imports for Cranpose applications.
pub mod prelude {
    pub use crate::{AppLauncher, AppSettings};
    pub use cranpose_core::{mutableStateOf, remember, rememberUpdatedState, useState};
    pub use cranpose_ui::*;
}

// Platform-specific runtime modules
#[cfg(all(feature = "android", feature = "renderer-wgpu"))]
pub mod android;

#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub mod desktop;

#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub mod recorder;

#[cfg(all(feature = "web", feature = "renderer-wgpu"))]
pub mod web;

// Re-export Robot type from desktop module when robot feature is enabled
#[cfg(all(feature = "desktop", feature = "renderer-wgpu", feature = "robot"))]
pub use desktop::{Robot, SemanticElement, SemanticRect};

/// FPS monitoring API - use these to track frame rate for performance optimization.
///
/// - `current_fps()` - Get current FPS value
/// - `fps_stats()` - Get detailed frame statistics (avg ms, recomps/sec)
/// - `fps_display()` - Get formatted FPS string for display
/// - `fps_display_detailed()` - Get detailed stats string
#[cfg(all(feature = "desktop", feature = "renderer-wgpu"))]
pub use cranpose_app_shell::{
    current_fps, fps_display, fps_display_detailed, fps_stats, DevOptions, FpsStats,
};
