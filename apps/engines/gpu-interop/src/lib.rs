//! wgpu objects made from another renderer's native GPU objects, so Cranpose
//! renders on that renderer's device and queue and shares its textures.
//!
//! This is the one Cranpose crate with unsafe code. Every unsafe block takes
//! a native handle from the engine that owns it, and the safe functions take
//! the engine's own objects, so callers never handle a raw pointer.

#![allow(unsafe_code)]

#[cfg(all(feature = "godot", target_vendor = "apple"))]
pub mod godot;
#[cfg(target_vendor = "apple")]
mod metal;

/// Why a native object could not become a wgpu object.
#[derive(Debug, thiserror::Error)]
pub enum InteropError {
    /// The engine renders with an API this crate does not import yet.
    #[error("rendering driver {0} is not supported yet")]
    UnsupportedDriver(String),
    /// The engine returned no handle for the object.
    #[error("the engine gave no native handle for the {0}")]
    MissingHandle(&'static str),
    /// No wgpu adapter runs on the engine's GPU.
    #[error("no wgpu adapter for the engine's GPU {0}")]
    NoAdapter(String),
    /// wgpu refused the device.
    #[error("device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    /// The texture has a format this crate does not map yet.
    #[error("texture format {0} is not supported yet")]
    UnsupportedFormat(String),
}

/// A native texture's size and format, read from the engine that owns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeTexture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The texture's format.
    pub format: wgpu::TextureFormat,
}
