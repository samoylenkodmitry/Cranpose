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
#[cfg(all(target_os = "android", target_arch = "arm"))]
mod aeabi_memory;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod app_info;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod app_update;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod camera;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
mod configuration;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod display_timing;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod event_loop;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod file_picker;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod finish;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod font_scale;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod frame_rate;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod frame_telemetry;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod frame_work;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod haptics_queue;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod heart_rate;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod host;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
mod host_window_requests;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
mod input;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod jni;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod keyboard;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod launch_args;
#[cfg(all(feature = "media-backend", target_os = "android"))]
pub(crate) mod media;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod microphone;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod microphone_standby;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod overlay_window;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod panic_hook;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod perf_hint;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod permissions;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
mod pipeline_cache;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod pipeline_update;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
mod pointer_icon;
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
mod shell_creation;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod surface;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod text_input;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod vsync;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod wearable;
#[cfg(all(feature = "webview", feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod webview;
#[cfg(any(test, feature = "renderer-wgpu"))]
pub(crate) mod wire_escape;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) mod writable_folder;

#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub use event_loop::*;
#[cfg(all(feature = "renderer-wgpu", target_os = "android"))]
pub(crate) use pipeline_cache::{keep_pipeline_cache_in, name_pipeline_cache_file};
