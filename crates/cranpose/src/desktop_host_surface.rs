//! The desktop window as the framework's host surface.
//!
//! A `winit` window is not `Send + Sync` and may only be touched from the event
//! loop, so the service holds no window: an application's resize request waits
//! in a queue the event loop drains on its next pass, exactly as the browser
//! host does with its canvas. One contract —
//! [`cranpose_services::HostSurface`] — the same shape on every platform.

use std::sync::{Arc, Mutex, OnceLock};

use cranpose_services::{HostSurface, ResizeRefused, set_platform_host_surface};

/// Wakes the event loop so a request made from an idle application is acted on
/// without waiting for the next frame something else caused.
type Wake = Arc<dyn Fn() + Send + Sync>;

struct DesktopHostSurface {
    wake: Wake,
}

fn pending() -> &'static Mutex<Option<(f32, f32)>> {
    static SLOT: OnceLock<Mutex<Option<(f32, f32)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

impl HostSurface for DesktopHostSurface {
    fn can_resize(&self) -> bool {
        true
    }

    fn request_size(&self, width: f32, height: f32) -> Result<(), ResizeRefused> {
        if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
            return Err(ResizeRefused::Rejected);
        }
        if let Ok(mut slot) = pending().lock() {
            *slot = Some((width, height));
        }
        (self.wake)();
        Ok(())
    }
}

/// Installs the desktop window as the platform host surface.
///
/// A window manager is free to clamp or ignore what is asked for, which is why
/// the request only reports that it was accepted; the size that took effect
/// arrives through the observable state like any other resize.
pub(crate) fn install(wake: impl Fn() + Send + Sync + 'static) {
    set_platform_host_surface(Arc::new(DesktopHostSurface {
        wake: Arc::new(wake),
    }));
}

/// Takes the size the application asked for, if it asked since the last call.
pub(crate) fn take_requested_size() -> Option<(f32, f32)> {
    pending().lock().ok().and_then(|mut slot| slot.take())
}
