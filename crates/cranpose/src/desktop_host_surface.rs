use std::sync::Arc;

use cranpose_services::{HostSurface, ResizeRefused, set_platform_host_surface};

pub(crate) use crate::host_surface_resize::take_requested_size;
use crate::host_surface_resize::validate_and_store;

type Wake = Arc<dyn Fn() + Send + Sync>;

struct DesktopHostSurface {
    wake: Wake,
}

impl HostSurface for DesktopHostSurface {
    fn can_resize(&self) -> bool {
        true
    }

    fn request_size(&self, width: f32, height: f32) -> Result<(), ResizeRefused> {
        validate_and_store(width, height)?;
        (self.wake)();
        Ok(())
    }
}

pub(crate) fn install(wake: impl Fn() + Send + Sync + 'static) {
    set_platform_host_surface(Arc::new(DesktopHostSurface {
        wake: Arc::new(wake),
    }));
}
