//! The DOM's `wheel` event, normalized into the shell's wheel sample.
//!
//! Two things separate a browser wheel event from what the shell dispatches,
//! and both are silent when wrong.
//!
//! **Units.** `WheelEvent` reports its delta in whichever of three units it
//! feels like — pixels, lines, or pages (`deltaMode`) — and only the pixel case
//! needs no work. A line is worth a notch; a page is worth the canvas.
//!
//! **Sign.** The DOM's `deltaY` is positive when the wheel is turned *down*
//! (the content moves up). The shell's convention, which winit already speaks,
//! is the opposite: positive means the content moves down and right. So the
//! browser delta is negated here, at the one point where the DOM's units and
//! signs are still visible. Without it every scrollable on the web runs
//! backwards, and a rotary handler turns the wrong way.

use cranpose_app_shell::{Modifiers, WheelScroll};
use cranpose_ui::Point;

/// Logical pixels one `DOM_DELTA_LINE` step scrolls.
const WEB_WHEEL_LINE_DELTA_PIXELS: f32 = 40.0;

/// The `deltaMode` values a `WheelEvent` can report, named as the DOM names
/// them. Mirrored rather than imported so this policy compiles (and its guards
/// run) on the host as well as on wasm.
pub(crate) const DOM_DELTA_PIXEL: u32 = 0;
pub(crate) const DOM_DELTA_LINE: u32 = 1;
pub(crate) const DOM_DELTA_PAGE: u32 = 2;

/// The size of one `DOM_DELTA_PAGE` step: the canvas's own CSS box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WebWheelPage {
    pub width: f32,
    pub height: f32,
}

/// Converts a raw DOM wheel sample into the shell's [`WheelScroll`].
pub(crate) fn wheel_scroll_from_dom(
    delta_x: f32,
    delta_y: f32,
    delta_mode: u32,
    page: WebWheelPage,
    modifiers: Modifiers,
    uptime_millis: u64,
) -> WheelScroll {
    let (unit_x, unit_y) = match delta_mode {
        DOM_DELTA_PIXEL => (1.0, 1.0),
        DOM_DELTA_LINE => (WEB_WHEEL_LINE_DELTA_PIXELS, WEB_WHEEL_LINE_DELTA_PIXELS),
        DOM_DELTA_PAGE => (page.width.max(1.0), page.height.max(1.0)),
        // Whatever a future browser invents: read it as pixels rather than
        // dropping the sample.
        _ => (1.0, 1.0),
    };

    WheelScroll::new(
        Point {
            x: -delta_x * unit_x,
            y: -delta_y * unit_y,
        },
        uptime_millis,
    )
    .with_modifiers(modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: WebWheelPage = WebWheelPage {
        width: 800.0,
        height: 600.0,
    };

    fn dom(delta_x: f32, delta_y: f32, delta_mode: u32) -> WheelScroll {
        wheel_scroll_from_dom(delta_x, delta_y, delta_mode, PAGE, Modifiers::NONE, 0)
    }

    #[test]
    fn a_wheel_turned_down_in_the_browser_scrolls_the_same_way_it_does_on_the_desktop() {
        // The DOM reports a wheel turned DOWN as a positive deltaY; the shell
        // and winit call that direction negative. This negation is the whole
        // reason the two hosts scroll the same way.
        let down = dom(0.0, 100.0, DOM_DELTA_PIXEL);
        let up = dom(0.0, -100.0, DOM_DELTA_PIXEL);

        assert_eq!(down.delta.y, -100.0);
        assert_eq!(up.delta.y, 100.0);
    }

    #[test]
    fn the_horizontal_axis_is_negated_with_the_vertical_one() {
        let right = dom(75.0, 0.0, DOM_DELTA_PIXEL);

        assert_eq!(right.delta.x, -75.0);
        assert_eq!(right.delta.y, 0.0);
    }

    #[test]
    fn line_and_page_modes_are_converted_to_pixels() {
        assert_eq!(dom(0.0, 3.0, DOM_DELTA_LINE).delta.y, -120.0);
        assert_eq!(dom(2.0, 0.0, DOM_DELTA_LINE).delta.x, -80.0);
        assert_eq!(dom(0.0, 1.0, DOM_DELTA_PAGE).delta.y, -PAGE.height);
        assert_eq!(dom(1.0, 0.0, DOM_DELTA_PAGE).delta.x, -PAGE.width);
    }

    #[test]
    fn a_degenerate_page_size_still_scrolls() {
        // A canvas can report a zero client box before layout settles; a page
        // scroll then has to move by something rather than silently nothing.
        let sample = wheel_scroll_from_dom(
            0.0,
            1.0,
            DOM_DELTA_PAGE,
            WebWheelPage {
                width: 0.0,
                height: 0.0,
            },
            Modifiers::NONE,
            0,
        );

        assert_eq!(sample.delta.y, -1.0);
    }

    #[test]
    fn an_unknown_delta_mode_is_read_as_pixels_rather_than_dropped() {
        assert_eq!(dom(0.0, 12.0, 99).delta.y, -12.0);
    }

    #[test]
    fn a_browser_pinch_arrives_as_the_zoom_gesture_and_zooms_in_when_spread() {
        // Trackpad pinches reach the page as ctrl+wheel, and a pinch OUT
        // (spread, zoom in) reports a negative deltaY -- which the negation
        // above turns into the positive delta the shell zooms in on.
        let spread = wheel_scroll_from_dom(
            0.0,
            -40.0,
            DOM_DELTA_PIXEL,
            PAGE,
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            0,
        );

        assert!(spread.is_zoom());
        assert!(spread.zoom_factor() > 1.0);
    }
}
