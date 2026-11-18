use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, SemanticsConfiguration, SemanticsWidgetRole, Size,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, DrawScope, Point, Stroke};

// The iOS 27 color well, measured on the simulator at 3x: 28 points across,
// a ring 3 points wide of every hue, a clear gap of 2 points, then the color
// in a circle of radius 9. The ring's hue turns clockwise from red at three
// o'clock through magenta, blue, cyan, green and yellow.
const SIDE: f32 = 28.0;
const RING_WIDTH: f32 = 3.0;
const SWATCH_RADIUS: f32 = 9.0;
const RING: [(u8, u8, u8); 12] = [
    (232, 74, 73),
    (201, 72, 165),
    (163, 71, 217),
    (113, 77, 232),
    (86, 159, 231),
    (75, 208, 203),
    (77, 231, 142),
    (122, 233, 95),
    (170, 229, 78),
    (227, 226, 76),
    (232, 186, 75),
    (232, 138, 74),
];

/// A swatch of `color` inside a ring of every hue. A tap calls `on_click`,
/// where the app shows a color picker; readers hear `label` as a button.
///
/// UIKit's `UIColorWell` and SwiftUI's `ColorPicker` as it sits closed.
#[composable]
pub fn LiquidColorWell(
    modifier: Modifier,
    label: &str,
    color: Color,
    on_click: impl Fn() + 'static,
) {
    let label = label.to_owned();
    let ring = cranpose_core::remember(ring_brush).with(Brush::clone);
    Box(
        modifier
            .size(Size::new(SIDE, SIDE))
            .clickable(move |_| on_click())
            .semantics(move |config: &mut SemanticsConfiguration| {
                config.role = Some(SemanticsWidgetRole::Button);
                config.content_description = Some(label.clone());
            })
            .draw_behind(move |scope| draw_well(scope, &ring, color)),
        BoxSpec::default(),
        || {},
    );
}

// The ring's brush, built once: the well's size is fixed.
fn ring_brush() -> Brush {
    Brush::sweep_gradient_stops(
        RING.iter()
            .chain(RING.first())
            .zip(0_u8..)
            .map(|(&(r, g, b), step)| (f32::from(step) / 12.0, Color::from_rgb_u8(r, g, b)))
            .collect(),
        Point::new(SIDE * 0.5, SIDE * 0.5),
    )
}

fn draw_well(scope: &mut dyn DrawScope, ring: &Brush, color: Color) {
    let center = Point::new(SIDE * 0.5, SIDE * 0.5);
    scope.draw_circle_stroked(
        ring.clone(),
        center,
        SIDE * 0.5 - RING_WIDTH * 0.5,
        Stroke::new(RING_WIDTH),
    );
    scope.draw_circle(Brush::solid(color), center, SWATCH_RADIUS);
}
