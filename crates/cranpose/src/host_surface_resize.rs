use std::sync::{Mutex, OnceLock};

use cranpose_services::ResizeRefused;

fn pending() -> &'static Mutex<Option<(f32, f32)>> {
    static SLOT: OnceLock<Mutex<Option<(f32, f32)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

pub(crate) fn validate_and_store(width: f32, height: f32) -> Result<(), ResizeRefused> {
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return Err(ResizeRefused::Rejected);
    }
    if let Ok(mut slot) = pending().lock() {
        *slot = Some((width, height));
    }
    Ok(())
}

pub(crate) fn take_requested_size() -> Option<(f32, f32)> {
    pending().lock().ok().and_then(|mut slot| slot.take())
}
