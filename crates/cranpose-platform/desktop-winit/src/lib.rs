#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui_graphics::Point;
use winit::{dpi::PhysicalPosition, event::MouseScrollDelta};

const LINE_SCROLL_DELTA_PIXELS: f32 = 40.0;

/// Converts winit window input into Cranpose pointer events in logical pixels.
pub struct DesktopWinitPlatform {
    scale_factor: f64,
}

impl DesktopWinitPlatform {
    /// Creates a platform adapter for a window with the given number of physical pixels per
    /// logical pixel.
    pub fn new(scale_factor: f64) -> Self {
        Self { scale_factor }
    }

    /// Sets the number of physical pixels per logical pixel, as winit reports it for the window.
    pub fn set_scale_factor(&mut self, factor: f64) {
        self.scale_factor = factor;
    }

    /// Converts a window position in physical pixels to a point in logical pixels.
    pub fn pointer_position(&self, position: PhysicalPosition<f64>) -> Point {
        Point {
            x: (position.x / self.scale_factor) as f32,
            y: (position.y / self.scale_factor) as f32,
        }
    }

    /// Builds a pointer event of `kind` at a window position given in physical pixels.
    ///
    /// The event carries the same logical position as its local and its root position.
    pub fn pointer_event(
        &self,
        kind: PointerEventKind,
        position: PhysicalPosition<f64>,
    ) -> PointerEvent {
        let logical = self.pointer_position(position);
        PointerEvent::new(kind, logical, logical)
    }

    /// Converts a winit scroll delta into a scroll offset in logical pixels.
    ///
    /// A line delta counts as 40 logical pixels per line. Returns `None` for delta kinds that
    /// winit may add later.
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
