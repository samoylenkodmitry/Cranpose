//! The browser canvas as the framework's host surface.
//!
//! The canvas lives on the browser thread and cannot be held by a `Send + Sync`
//! service, so the service keeps only numbers: the resize the application asked
//! for waits in a queue the render loop drains, and the size the canvas ends up
//! with is published like every other host's. That keeps one contract —
//! [`cranpose_services::HostSurface`] — the same shape on every platform.

use std::sync::{Arc, Mutex, OnceLock};

use cranpose_services::{set_platform_host_surface, HostSurface, HostSurfaceSize, ResizeRefused};

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

/// Installs the browser canvas as the platform host surface.
pub(crate) fn install() {
    set_platform_host_surface(Arc::new(WebHostSurface));
}

/// Publishes the size the canvas actually has now.
pub(crate) fn publish(width: f32, height: f32, scale: f32) {
    cranpose_services::publish_host_surface_size(HostSurfaceSize {
        width,
        height,
        scale,
    });
}

/// Takes the size the application asked for, if it asked since the last call.
pub(crate) fn take_requested_size() -> Option<(f32, f32)> {
    pending().lock().ok().and_then(|mut slot| slot.take())
}
