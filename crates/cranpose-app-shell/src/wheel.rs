use cranpose_foundation::Modifiers;
use cranpose_ui_graphics::Point;

const NOTCH_LOGICAL_PX: f32 = 40.0;
const ZOOM_PER_NOTCH: f32 = 1.2;

/// A mouse-wheel or trackpad scroll sample ready for
/// [`AppShell::wheel_scrolled`](crate::AppShell::wheel_scrolled).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelScroll {
    /// Scroll amount in logical pixels; positive moves content down and right
    /// (see the module docs — this is winit's sign, not the DOM's).
    pub delta: Point,
    /// Keyboard modifiers held during the sample. `ctrl` makes it a zoom
    /// gesture, `alt` turns a vertical wheel into a horizontal scroll.
    pub modifiers: Modifiers,
    /// Monotonic milliseconds, for the rotary event's velocity tracking. Only
    /// differences between samples are meaningful.
    pub uptime_millis: u64,
}

impl WheelScroll {
    /// A sample with no modifiers held.
    pub fn new(delta: Point, uptime_millis: u64) -> Self {
        Self {
            delta,
            modifiers: Modifiers::NONE,
            uptime_millis,
        }
    }

    /// This sample with `modifiers` held.
    pub fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Whether this sample is the zoom gesture (ctrl+wheel, which is also how
    /// trackpad pinches arrive in a browser) rather than a scroll.
    pub fn is_zoom(&self) -> bool {
        self.modifiers.ctrl
    }

    /// The multiplicative zoom step for a ctrl+wheel sample: one notch up
    /// (positive delta) zooms in by `ZOOM_PER_NOTCH`.
    pub fn zoom_factor(&self) -> f32 {
        ZOOM_PER_NOTCH.powf(self.delta.y / NOTCH_LOGICAL_PX)
    }

    /// The delta the hovered scrollable should see: with alt held, a vertical
    /// wheel drives the horizontal axis instead (the shift-less way to scroll
    /// a row on a wheel that only has a vertical axis).
    pub fn scroll_delta(&self) -> Point {
        if !self.modifiers.alt {
            return self.delta;
        }
        let x = if self.delta.x.abs() <= f32::EPSILON {
            self.delta.y
        } else {
            self.delta.x
        };
        Point { x, y: 0.0 }
    }
}

#[cfg(test)]
#[path = "tests/wheel_tests.rs"]
mod tests;
