#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui_graphics::Point;

/// Converts browser pointer input into Cranpose pointer events.
pub struct WebPlatform {
    scale_factor: f64,
}

impl WebPlatform {
    /// Creates a platform adapter for a canvas with the given device pixel ratio.
    pub fn new(scale_factor: f64) -> Self {
        Self { scale_factor }
    }

    /// Sets the device pixel ratio of the canvas.
    ///
    /// Pointer positions are not scaled by it, because browser events use CSS pixels.
    pub fn set_scale_factor(&mut self, factor: f64) {
        self.scale_factor = factor;
    }

    /// Returns the canvas position `x`, `y` as a `Point` in logical pixels.
    ///
    /// Browser events already use CSS pixels, so the position is not scaled.
    pub fn pointer_position(&self, x: f64, y: f64) -> Point {
        Point {
            x: x as f32,
            y: y as f32,
        }
    }

    /// Builds a pointer event of `kind` at a canvas position given in CSS pixels.
    ///
    /// The event carries the same position as its local and its root position.
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
