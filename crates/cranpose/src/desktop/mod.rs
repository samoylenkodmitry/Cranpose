//! Desktop runtime for Compose applications.
//!
//! This module provides the desktop event loop implementation using winit.

pub(crate) mod accessibility;
pub(crate) mod accessibility_options;
pub(crate) mod bundled_assets;
pub(crate) mod cursor;
pub(crate) mod event_loop;
pub(crate) mod host_surface;
pub(crate) mod incoming;
pub(crate) mod input;
pub(crate) mod integration;
pub(crate) mod lifecycle;
#[cfg(target_os = "macos")]
pub(crate) mod macos_cursor;
#[cfg(target_os = "macos")]
pub(crate) mod macos_menu;
#[cfg(target_os = "macos")]
pub(crate) mod macos_open;
#[cfg(target_os = "macos")]
pub(crate) mod macos_power;
#[cfg(target_os = "macos")]
pub(crate) mod macos_window;
#[cfg(all(
    feature = "desktop-menus",
    any(
        target_os = "macos",
        target_os = "windows",
        all(target_os = "linux", not(target_arch = "wasm32"))
    )
))]
pub(crate) mod menus;
pub(crate) mod power;
#[cfg(target_os = "windows")]
pub(crate) mod windows_cursor;
#[cfg(all(feature = "desktop-menus", target_os = "windows"))]
pub(crate) mod windows_menu;
#[cfg(target_os = "windows")]
pub(crate) mod windows_network;

pub use event_loop::*;

#[cfg(feature = "media-backend")]
mod media;
