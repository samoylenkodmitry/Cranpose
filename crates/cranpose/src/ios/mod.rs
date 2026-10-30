//! iOS runtime for Cranpose applications.
//!
//! This backend is built on winit's UIKit integration (`winit-uikit`), which
//! provides a real `UIWindow`/`UIView` backed by a `CAMetalLayer`, CADisplayLink
//! driven redraw scheduling, touch input, density (scale factor) and the
//! application lifecycle. It is a native iOS surface, not a reuse of the desktop
//! window backend: the event loop here only manages a single fullscreen surface
//! and maps UIKit touches to Cranpose pointer input.
//!
//! winit starts `UIApplicationMain` from [`EventLoop::run_app`], so the iOS app
//! binary needs no Objective-C entry point.

#[cfg(feature = "renderer-wgpu")]
pub(crate) mod accessibility;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod app_info;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod back_gesture;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod background;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod bundled_assets;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod clipboard;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod device_info;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod downloads;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod event_loop;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod file_picker;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod haptics;
pub(crate) mod host;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod image_picker;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod keyboard;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod media;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod media_artwork;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod notifier;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod pick_future;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod scene;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod share_sheet;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod storage;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod uri_handler;
#[cfg(feature = "renderer-wgpu")]
pub(crate) mod writable_folder;

#[cfg(feature = "renderer-wgpu")]
pub use event_loop::*;
