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
mod tests {
    use super::*;

    fn wheel(x: f32, y: f32) -> WheelScroll {
        WheelScroll::new(Point { x, y }, 0)
    }

    #[test]
    fn a_plain_sample_is_neither_a_zoom_nor_axis_swapped() {
        let sample = wheel(3.0, -40.0);

        assert!(!sample.is_zoom());
        assert_eq!(sample.scroll_delta(), sample.delta);
    }

    #[test]
    fn ctrl_makes_a_sample_a_zoom_that_grows_when_the_wheel_turns_up() {
        let up = wheel(0.0, NOTCH_LOGICAL_PX).with_modifiers(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        });
        let down = wheel(0.0, -NOTCH_LOGICAL_PX).with_modifiers(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        });

        assert!(up.is_zoom());
        assert!((up.zoom_factor() - ZOOM_PER_NOTCH).abs() < 1.0e-6);
        assert!((up.zoom_factor() * down.zoom_factor() - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn alt_moves_a_vertical_wheel_onto_the_horizontal_axis() {
        let alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };

        assert_eq!(
            wheel(0.0, 48.0).with_modifiers(alt).scroll_delta(),
            Point { x: 48.0, y: 0.0 }
        );
        assert_eq!(
            wheel(12.0, 48.0).with_modifiers(alt).scroll_delta(),
            Point { x: 12.0, y: 0.0 }
        );
    }
}
