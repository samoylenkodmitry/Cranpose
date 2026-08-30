use std::sync::Arc;

use cranpose_services::{HostSurface, HostSurfaceSize, ResizeRefused, set_platform_host_surface};

pub(crate) use crate::host_surface_resize::take_requested_size;
use crate::host_surface_resize::validate_and_store;

struct WebHostSurface;

impl HostSurface for WebHostSurface {
    fn can_resize(&self) -> bool {
        true
    }

    fn request_size(&self, width: f32, height: f32) -> Result<(), ResizeRefused> {
        validate_and_store(width, height)
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
