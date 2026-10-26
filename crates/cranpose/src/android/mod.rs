//! Android runtime for Compose applications.
//!
//! This module provides the Android event loop implementation with proper
//! lifecycle management, input handling, and rendering coordination.

#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod accessibility;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod accessibility_wire;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
mod activity_window_layout;
#[cfg(target_os = "android")]
pub(crate) mod app_info;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod app_update;
#[cfg(target_os = "android")]
pub(crate) mod camera;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod display_timing;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod event_loop;
#[cfg(target_os = "android")]
pub(crate) mod file_picker;
#[cfg(target_os = "android")]
pub(crate) mod finish;
#[cfg(target_os = "android")]
pub(crate) mod font_scale;
#[cfg(target_os = "android")]
pub(crate) mod frame_rate;
#[cfg(target_os = "android")]
pub(crate) mod frame_telemetry;
pub(crate) mod frame_work;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod haptics_queue;
#[cfg(target_os = "android")]
pub(crate) mod heart_rate;
#[cfg(target_os = "android")]
pub(crate) mod host;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
mod host_window_requests;
#[cfg(target_os = "android")]
pub(crate) mod jni;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod keyboard;
pub(crate) mod launch_args;
#[cfg(all(feature = "media-backend", target_os = "android"))]
pub(crate) mod media;
#[cfg(target_os = "android")]
pub(crate) mod microphone;
#[cfg(target_os = "android")]
pub(crate) mod microphone_standby;
#[cfg(target_os = "android")]
pub(crate) mod overlay_window;
pub(crate) mod panic_hook;
#[cfg(target_os = "android")]
pub(crate) mod perf_hint;
#[cfg(target_os = "android")]
pub(crate) mod permissions;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod pipeline_update;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod poll;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod present_thread;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod process;
#[cfg(any(test, feature = "playbilling"))]
pub(crate) mod purchase_wire;
#[cfg(all(
    feature = "playbilling",
    feature = "renderer-wgpu",
    target_os = "android"
))]
pub(crate) mod purchases;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod services;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod surface;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod text_input;
#[cfg(target_os = "android")]
pub(crate) mod vsync;
#[cfg(target_os = "android")]
pub(crate) mod wearable;
#[cfg(all(feature = "webview", feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod webview;
pub(crate) mod wire_escape;
#[cfg(target_os = "android")]
pub(crate) mod writable_folder;

#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub use event_loop::*;
