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

    pub fn scroll_delta(&self, delta: MouseScrollDelta) -> Point {
        match delta {
            MouseScrollDelta::LineDelta(x, y) => Point {
                x: x * LINE_SCROLL_DELTA_PIXELS,
                y: y * LINE_SCROLL_DELTA_PIXELS,
            },
            MouseScrollDelta::PixelDelta(delta) => Point {
                x: (delta.x / self.scale_factor) as f32,
                y: (delta.y / self.scale_factor) as f32,
            },
        }
    }
}

impl Default for DesktopWinitPlatform {
    fn default() -> Self {
        Self::new(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointer_event_carries_the_logical_position_as_both_of_its_points() {
        let platform = DesktopWinitPlatform::new(2.0);
        let event = platform.pointer_event(
            PointerEventKind::Move,
            PhysicalPosition { x: 100.0, y: 50.0 },
        );
        assert_eq!(event.position, Point { x: 50.0, y: 25.0 });
        assert_eq!(event.position, event.global_position);
        assert_eq!(event.kind, PointerEventKind::Move);
    }

    #[test]
    fn line_scroll_delta_is_scaled_to_pixels() {
        let platform = DesktopWinitPlatform::new(1.0);
        let delta = platform.scroll_delta(MouseScrollDelta::LineDelta(1.0, -2.0));
        assert_eq!(delta.x, 40.0);
        assert_eq!(delta.y, -80.0);
    }

    #[test]
    fn pixel_scroll_delta_is_converted_to_logical_space() {
        let platform = DesktopWinitPlatform::new(2.0);
        let delta = platform.scroll_delta(MouseScrollDelta::PixelDelta(PhysicalPosition {
            x: 24.0,
            y: -10.0,
        }));
        assert_eq!(delta.x, 12.0);
        assert_eq!(delta.y, -5.0);
    }
}
