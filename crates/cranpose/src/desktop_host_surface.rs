use std::sync::{Arc, Mutex, OnceLock};

use cranpose_services::{HostSurface, ResizeRefused, set_platform_host_surface};

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

pub(crate) fn install(wake: impl Fn() + Send + Sync + 'static) {
    set_platform_host_surface(Arc::new(DesktopHostSurface {
        wake: Arc::new(wake),
    }));
}

pub(crate) fn take_requested_size() -> Option<(f32, f32)> {
    pending().lock().ok().and_then(|mut slot| slot.take())
}
