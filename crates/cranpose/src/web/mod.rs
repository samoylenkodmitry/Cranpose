//! Web runtime for Compose applications.
//!
//! This module provides the web event loop implementation using wasm-bindgen and WebGPU.

#[cfg(target_arch = "wasm32")]
pub(crate) mod accessibility;
pub(crate) mod accessibility_attributes;
#[cfg(target_arch = "wasm32")]
pub(crate) mod accessibility_options;
pub(crate) mod accessibility_order;
#[cfg(target_arch = "wasm32")]
mod backend_preference;
pub(crate) mod canvas_layout;
#[cfg(target_arch = "wasm32")]
pub(crate) mod canvas_watch;
#[cfg(target_arch = "wasm32")]
pub(crate) mod clipboard;
#[cfg(target_arch = "wasm32")]
pub(crate) mod cursor;
#[cfg(target_arch = "wasm32")]
pub(crate) mod device_loss;
#[cfg(target_arch = "wasm32")]
pub(crate) mod drop;
#[cfg(target_arch = "wasm32")]
pub(crate) mod event_loop;
pub(crate) mod floating_window;
#[cfg(target_arch = "wasm32")]
pub(crate) mod frame_host;
#[cfg(target_arch = "wasm32")]
pub(crate) mod history;
#[cfg(target_arch = "wasm32")]
pub(crate) mod host_surface;
#[cfg(target_arch = "wasm32")]
pub(crate) mod idle_work;
#[cfg(target_arch = "wasm32")]
mod keys;
#[cfg(target_arch = "wasm32")]
pub(crate) mod lifecycle;
#[cfg(target_arch = "wasm32")]
pub(crate) mod media;
#[cfg(target_arch = "wasm32")]
pub(crate) mod pipeline_discovery;
#[cfg(target_arch = "wasm32")]
pub(crate) mod pointer;
#[cfg(target_arch = "wasm32")]
pub(crate) mod power;
#[cfg(target_arch = "wasm32")]
pub(crate) mod services;
pub(crate) mod surface_scale;
#[cfg(target_arch = "wasm32")]
pub(crate) mod text_input;
pub(crate) mod text_span;
#[cfg(all(feature = "webview", target_arch = "wasm32"))]
pub(crate) mod webview;
pub(crate) mod wheel;

#[cfg(target_arch = "wasm32")]
pub use event_loop::*;
