//! One mouse-wheel / trackpad sample, in the shell's wheel convention.
//!
//! Every host that has a wheel — the winit desktop loop, the browser's `wheel`
//! listener — has to answer the same four questions in the same order: is this
//! a zoom gesture, does a rotary handler want it, is it an axis-swapped
//! horizontal scroll, and otherwise how much does the hovered scrollable move.
//! [`AppShell::wheel_scrolled`](crate::AppShell::wheel_scrolled) is that answer,
//! and this type is its input, so a host's whole job is normalizing its native
//! event into a [`WheelScroll`].
//!
//! # Sign convention
//!
//! `delta` is **logical pixels, positive when the content being scrolled should
//! move down and right** — the direction a wheel turned up / away from the user
//! produces. That is winit's convention and the one the scroll modifiers are
//! written against (a positive vertical delta walks a `ScrollState` back toward
//! zero). The DOM's is the opposite: `WheelEvent::delta_y` is positive when the
//! wheel is turned *down*, so the browser host negates on the way in. Getting
//! this wrong does not fail loudly — it scrolls backwards.

use cranpose_foundation::Modifiers;
use cranpose_ui_graphics::Point;

/// Logical pixels one wheel notch scrolls, and the notch the ctrl+wheel zoom
/// step is defined against.
const NOTCH_LOGICAL_PX: f32 = 40.0;
/// Zoom applied by one ctrl+wheel notch.
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
        // Zooming out by a notch must undo zooming in by one, or a pinch
        // in-and-out drifts the scale.
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
        // A device that already reports a horizontal axis keeps it: only the
        // vertical-only wheel needs the swap.
        assert_eq!(
            wheel(12.0, 48.0).with_modifiers(alt).scroll_delta(),
            Point { x: 12.0, y: 0.0 }
        );
    }
}
