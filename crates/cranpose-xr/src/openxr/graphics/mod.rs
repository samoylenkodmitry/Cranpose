//! The graphics API of a headset's session: GLES on Android and Vulkan on Linux and Windows.
//!
//! Each API's module offers the same items under the same names: the `Graphics` that makes the
//! session and Cranpose's wgpu device, the `Api` that OpenXR's types take, and the wrapping of a
//! swapchain's images as wgpu textures. Everything else in the backend works on these names.

#[cfg(target_os = "android")]
mod gles;
mod swapchain_format;
mod texture_descriptors;
#[cfg(not(target_os = "android"))]
mod vulkan;

#[cfg(target_os = "android")]
pub(crate) use gles::{Api, Graphics};
pub(crate) use swapchain_format::SwapchainFormat;
#[cfg(not(target_os = "android"))]
pub(crate) use vulkan::{Api, Graphics};
