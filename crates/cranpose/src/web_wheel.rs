use cranpose_app_shell::{Modifiers, WheelScroll};
use cranpose_ui::Point;

const WEB_WHEEL_LINE_DELTA_PIXELS: f32 = 40.0;

pub(crate) const DOM_DELTA_PIXEL: u32 = 0;
pub(crate) const DOM_DELTA_LINE: u32 = 1;
pub(crate) const DOM_DELTA_PAGE: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WebWheelPage {
    pub width: f32,
    pub height: f32,
}

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
#[path = "tests/web_wheel_tests.rs"]
mod tests;
