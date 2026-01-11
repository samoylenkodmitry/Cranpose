#![deny(missing_docs)]

//! High level utilities for running Compose applications with minimal boilerplate.

#[cfg(not(any(feature = "desktop", feature = "android", feature = "web")))]
compile_error!(
    "compose-app must be built with at least one of `desktop`, `android`, or `web` features."
);

#[cfg(not(any(feature = "renderer-pixels", feature = "renderer-wgpu")))]
compile_error!("compose-app requires either `renderer-pixels` or `renderer-wgpu` feature.");

mod launcher;
pub use launcher::{AppLauncher, AppSettings};

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
pub use compose_app_shell::{
    current_fps, fps_display, fps_display_detailed, fps_stats, DevOptions, FpsStats,
};
