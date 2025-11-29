use compose_foundation::{PointerButtons, PointerEvent, PointerEventKind, PointerPhase};
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
        Point {
            x: (x / self.scale_factor) as f32,
            y: (y / self.scale_factor) as f32,
        }
    }

    pub fn pointer_event(
        &self,
        kind: PointerEventKind,
        x: f64,
        y: f64,
    ) -> PointerEvent {
        let logical = self.pointer_position(x, y);
        PointerEvent {
            id: 0,
            kind,
            phase: match kind {
                PointerEventKind::Down => PointerPhase::Start,
                PointerEventKind::Move => PointerPhase::Move,
                PointerEventKind::Up => PointerPhase::End,
                PointerEventKind::Cancel => PointerPhase::Cancel,
            },
            position: logical,
            global_position: logical,
            buttons: PointerButtons::NONE,
        }
    }
}

impl Default for WebPlatform {
    fn default() -> Self {
        Self::new(1.0)
    }
}
