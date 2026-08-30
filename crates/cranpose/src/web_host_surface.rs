use std::sync::{Arc, Mutex, OnceLock};

use cranpose_services::{HostSurface, HostSurfaceSize, ResizeRefused, set_platform_host_surface};

fn pending() -> &'static Mutex<Option<(f32, f32)>> {
    static SLOT: OnceLock<Mutex<Option<(f32, f32)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

struct WebHostSurface;

impl HostSurface for WebHostSurface {
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
        Ok(())
    }
}

pub(crate) fn install() {
    set_platform_host_surface(Arc::new(WebHostSurface));
}

pub(crate) fn publish(width: f32, height: f32, scale: f32) {
    cranpose_services::publish_host_surface_size(HostSurfaceSize {
        width,
        height,
        scale,
    });
}

pub(crate) fn take_requested_size() -> Option<(f32, f32)> {
    pending().lock().ok().and_then(|mut slot| slot.take())
}
