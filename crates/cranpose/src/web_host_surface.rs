use std::{rc::Rc, sync::Arc};

use cranpose_services::{HostSurface, HostSurfaceSize, ResizeRefused, set_platform_host_surface};

pub(crate) use crate::host_surface_resize::take_requested_size;
use crate::host_surface_resize::validate_and_store;

struct WebHostSurface;

impl HostSurface for WebHostSurface {
    fn can_resize(&self) -> bool {
        true
    }

    fn request_size(&self, width: f32, height: f32) -> Result<(), ResizeRefused> {
        validate_and_store(width, height)?;
        crate::thread_state::with(|state| {
            if let Some(wake) = state.web.host_surface_wake.borrow().as_ref() {
                wake();
            }
        });
        Ok(())
    }
}

/// Asks for a frame whenever the app requests a size, so a request made
/// between frames is applied in the next one rather than waiting for one.
pub(crate) fn wake_with(wake: Rc<dyn Fn()>) {
    crate::thread_state::with(|state| *state.web.host_surface_wake.borrow_mut() = Some(wake));
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
