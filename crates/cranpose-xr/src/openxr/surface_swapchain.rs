//! A swapchain that an Android surface backs, which a decoder or a camera writes into.
#![expect(
    unsafe_code,
    reason = "xrCreateSwapchainAndroidSurfaceKHR, its Java surface and the swapchain handle"
)]

use std::ptr;

use cranpose::{__android_jni::JavaVM, ExternalSurface};
use openxr::{self as xr, sys::Handle};

use super::graphics::Api;

// Makes a surface swapchain whose surface has buffers of `size` pixels until the producer picks
// its own, and the surface that writes into it. The compositor shows what the producer posts
// with no copy, and the app does not acquire or release the swapchain's images.
pub(crate) fn create(
    session: &xr::Session<Api>,
    size: (u32, u32),
) -> Result<(xr::Swapchain<Api>, ExternalSurface), String> {
    let create = session
        .instance()
        .exts()
        .khr_android_surface_swapchain
        .ok_or("the OpenXR runtime has no XR_KHR_android_surface_swapchain")?
        .create_swapchain_android_surface;
    // The extension takes only the size and the usage: the surface's producer decides the rest.
    let info = xr::sys::SwapchainCreateInfo {
        ty: xr::sys::SwapchainCreateInfo::TYPE,
        next: ptr::null(),
        create_flags: xr::SwapchainCreateFlags::EMPTY,
        usage_flags: xr::SwapchainUsageFlags::EMPTY,
        format: 0,
        sample_count: 0,
        width: size.0,
        height: size.1,
        face_count: 0,
        array_size: 0,
        mip_count: 0,
    };
    let mut handle = xr::sys::Swapchain::NULL;
    let mut java_surface: xr::sys::platform::jobject = ptr::null_mut();
    // SAFETY: the function belongs to the session's instance, which enabled the extension. The
    // out pointers name live locals.
    let result = unsafe { create(session.as_raw(), &info, &mut handle, &mut java_surface) };
    if result != xr::sys::Result::SUCCESS || java_surface.is_null() {
        return Err(format!("OpenXR surface swapchain: {result:?}"));
    }
    // SAFETY: the handle is a new swapchain of this session. Its owner destroys it, also when
    // the surface below fails.
    let swapchain = unsafe { xr::Swapchain::from_raw(session.clone(), handle) };
    Ok((swapchain, native_surface(java_surface)?))
}

// The surface of the Java `Surface` that the runtime returned. The runtime owns that object. The
// native window that this takes holds its own reference.
fn native_surface(java_surface: xr::sys::platform::jobject) -> Result<ExternalSurface, String> {
    let vm = JavaVM::singleton().map_err(|error| format!("Android JavaVM: {error}"))?;
    vm.attach_current_thread(
        |env| -> Result<Option<ExternalSurface>, cranpose::__android_jni::errors::Error> {
            // SAFETY: the environment belongs to this thread, and the runtime returned a valid
            // `Surface` reference.
            Ok(unsafe { ExternalSurface::from_java_surface(env.get_raw().cast(), java_surface) })
        },
    )
    .map_err(|error| format!("Android JNI: {error}"))?
    .ok_or_else(|| "the surface swapchain has no native window".to_string())
}
