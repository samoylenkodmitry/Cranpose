#![allow(unsafe_code)]

use std::{ffi::c_void, ptr::NonNull};

#[derive(Debug, thiserror::Error)]
pub(crate) enum AndroidSurfaceError {
    #[error("failed to create Android WGPU surface: {0}")]
    CreateSurface(#[from] wgpu::CreateSurfaceError),
    #[error("failed to find suitable Android WGPU adapter: {0}")]
    RequestAdapter(#[from] wgpu::RequestAdapterError),
    #[error("failed to create Android WGPU device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("Android WGPU surface reports no supported formats")]
    NoSurfaceFormat,
    #[error("Android WGPU surface reports no supported alpha modes")]
    NoAlphaMode,
    #[error("failed to start Android present runtime: {0}")]
    PresentRuntime(String),
}

pub(crate) fn create_android_wgpu_surface(
    instance: &wgpu::Instance,
    native_window_ptr: NonNull<c_void>,
) -> Result<wgpu::Surface<'static>, AndroidSurfaceError> {
    use raw_window_handle::{
        AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
    };

    let window_handle = AndroidNdkWindowHandle::new(native_window_ptr);
    let raw_window_handle = RawWindowHandle::AndroidNdk(window_handle);
    let display_handle = AndroidDisplayHandle::new();
    let raw_display_handle = RawDisplayHandle::Android(display_handle);

    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(raw_display_handle),
        raw_window_handle,
    };

    unsafe { instance.create_surface_unsafe(target) }.map_err(AndroidSurfaceError::from)
}
