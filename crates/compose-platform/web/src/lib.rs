use compose_foundation::{PointerEvent, PointerEventKind};
use compose_ui_graphics::Point;

pub struct WebPlatform {
    scale_factor: f64,
}

impl WebPlatform {
    pub fn new(scale_factor: f64) -> Self {
        Self { scale_factor }
    }

    pub fn set_scale_factor(&mut self, factor: f64) {
        self.scale_factor = factor;
    }

    pub fn pointer_position(&self, x: f64, y: f64) -> Point {
        // offset_x/offset_y are already in CSS pixels (logical coordinates)
        // so we don't need to divide by scale_factor
        Point {
            x: x as f32,
            y: y as f32,
        }
    }

    pub fn pointer_event(&self, kind: PointerEventKind, x: f64, y: f64) -> PointerEvent {
        let logical = self.pointer_position(x, y);
        PointerEvent::new(kind, logical, logical)
    }
}

impl Default for WebPlatform {
    fn default() -> Self {
        Self::new(1.0)
    }
}
