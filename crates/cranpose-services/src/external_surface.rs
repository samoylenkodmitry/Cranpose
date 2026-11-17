//! A surface that a producer on another thread writes pictures into.
//!
//! A hardware video decoder or a camera writes its pictures into an [`ExternalSurface`]. The
//! compositor of the host shows them with no copy through the app. In a headset,
//! `SpatialExternalSurface` of `cranpose-xr` gives the app one. A service that produces
//! pictures takes the surface. [`DecoderOutput::Surface`](crate::DecoderOutput::Surface) sends the
//! pictures of a [`VideoDecoder`](crate::VideoDecoder) into one. A producer of another kind reads
//! the native handle with [`ExternalSurface::as_ptr`].
#![cfg_attr(
    target_os = "android",
    expect(
        unsafe_code,
        reason = "ANativeWindow_fromSurface takes a JNI environment and a Java object"
    )
)]

use std::{ffi::c_void, fmt};

#[cfg(target_os = "android")]
type Window = ndk::native_window::NativeWindow;

// Only a headset's compositor on Android shows an external surface. Other platforms have none,
// so the type has no values there.
#[cfg(not(target_os = "android"))]
type Window = std::convert::Infallible;

/// A surface that a producer writes pictures into, such as a hardware video decoder or a camera.
///
/// On Android it holds one reference to the surface's native window (`ANativeWindow`). A
/// clone holds another. The window ends when the last clone drops and the owner of the surface
/// ends it, so a producer that outlives the owner fails its writes without harm. A thread may
/// own a clone.
///
/// Other platforms have no surfaces of this kind, so no value of the type exists there.
#[derive(Clone)]
pub struct ExternalSurface {
    window: Window,
}

impl ExternalSurface {
    /// The surface's native window as a pointer: on Android an `ANativeWindow*`, which
    /// `AMediaCodec_configure` and `ANativeWindow_lock` take. The pointer stays valid while this
    /// value or a clone of it lives.
    pub fn as_ptr(&self) -> *mut c_void {
        #[cfg(target_os = "android")]
        {
            self.window.ptr().as_ptr().cast()
        }
        #[cfg(not(target_os = "android"))]
        match self.window {}
    }

    // The surface's native window, for a decoder in this crate that configures its codec with it.
    #[cfg(target_os = "android")]
    pub(crate) fn window(&self) -> &ndk::native_window::NativeWindow {
        &self.window
    }

    /// Takes the native window of the Java `android.view.Surface` named by `surface`, or
    /// returns `None` if the surface has none.
    ///
    /// # Safety
    ///
    /// `env` must point to the `JNIEnv` of the calling thread, and `surface` must be a valid
    /// reference to a `android.view.Surface` in it.
    #[cfg(target_os = "android")]
    pub unsafe fn from_java_surface(env: *mut c_void, surface: *mut c_void) -> Option<Self> {
        // SAFETY: the caller guarantees the environment and the surface.
        let window =
            unsafe { ndk::native_window::NativeWindow::from_surface(env.cast(), surface.cast()) }?;
        Some(Self { window })
    }
}

// Two values are equal when they hold the same native window.
impl PartialEq for ExternalSurface {
    fn eq(&self, other: &Self) -> bool {
        self.as_ptr() == other.as_ptr()
    }
}

impl Eq for ExternalSurface {}

impl fmt::Debug for ExternalSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalSurface").finish_non_exhaustive()
    }
}
