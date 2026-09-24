use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui_graphics::Point;
use winit::{dpi::PhysicalPosition, event::MouseScrollDelta};

const LINE_SCROLL_DELTA_PIXELS: f32 = 40.0;

pub struct DesktopWinitPlatform {
    scale_factor: f64,
}

impl DesktopWinitPlatform {
    pub fn new(scale_factor: f64) -> Self {
        Self { scale_factor }
    }

    pub fn set_scale_factor(&mut self, factor: f64) {
        self.scale_factor = factor;
    }

    pub fn pointer_position(&self, position: PhysicalPosition<f64>) -> Point {
        Point {
            x: (position.x / self.scale_factor) as f32,
            y: (position.y / self.scale_factor) as f32,
        }
    }

    pub fn pointer_event(
        &self,
        kind: PointerEventKind,
        position: PhysicalPosition<f64>,
    ) -> PointerEvent {
        let logical = self.pointer_position(position);
        PointerEvent::new(kind, logical, logical)
    }

    pub fn scroll_delta(&self, delta: MouseScrollDelta) -> Option<Point> {
        match delta {
            MouseScrollDelta::LineDelta(x, y) => Some(Point {
                x: x * LINE_SCROLL_DELTA_PIXELS,
                y: y * LINE_SCROLL_DELTA_PIXELS,
            }),
            MouseScrollDelta::PixelDelta(delta) => Some(Point {
                x: (delta.x / self.scale_factor) as f32,
                y: (delta.y / self.scale_factor) as f32,
            }),
            _ => None,
        }
    }
}

impl Default for DesktopWinitPlatform {
    fn default() -> Self {
        Self::new(1.0)
    }
}

#[cfg(test)]
#[path = "tests/desktop_winit_tests.rs"]
mod tests;
