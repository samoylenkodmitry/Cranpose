use std::{ops::RangeInclusive, rc::Rc};

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, Point, SemanticsConfiguration, SemanticsWidgetRole, Size,
    widgets::{Box, BoxSpec, Row, RowSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, Rect, Stroke, StrokeCap};

use crate::theme::liquid_colors;

// The iOS 27 stepper, measured on the simulator at 3x: a 93 by 32 capsule
// split by a 1 point rule 24 points tall, a minus and a plus 40/3 points
// long and 5/3 points thick, each a little left of its half's center.
const WIDTH: f32 = 93.0;
const HEIGHT: f32 = 32.0;
const RULE_WIDTH: f32 = 1.0;
const RULE_INSET: f32 = 4.0;
const GLYPH_LENGTH: f32 = 40.0 / 3.0;
const GLYPH_STROKE: f32 = 5.0 / 3.0;
const MINUS_CENTER_X: f32 = 23.0;
const PLUS_CENTER_X: f32 = 209.0 / 3.0;
const GLYPH_CENTER_Y: f32 = 16.0;

// The capsule's fill, fitted over the checkerboard: a dark gray at 8.6% that
// shades a light screen, and a light gray at 7.7% that lifts a dark one.
fn capsule_fill(dark: bool) -> Color {
    if dark {
        Color::from_rgba_u8(231, 231, 241, 20)
    } else {
        Color::from_rgba_u8(58, 58, 71, 22)
    }
}

/// A pair of buttons that lower and raise `value` by `step` within `range`.
/// `value` is owned by the caller; `on_change` receives the requested new
/// value. A button whose step would leave `range` is disabled and dims, and
/// readers hear "Decrement" and "Increment" with the value.
///
/// UIKit's `UIStepper` and SwiftUI's `Stepper`.
#[composable]
pub fn LiquidStepper(
    modifier: Modifier,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    on_change: impl Fn(f32) + 'static,
) {
    let colors = liquid_colors();
    let fill = capsule_fill(colors.is_dark);
    let rule = colors.tertiary_label;
    let on_change: Rc<dyn Fn(f32)> = Rc::new(on_change);
    let (low, high) = (*range.start(), *range.end());
    let lower = (value - step).max(low);
    let higher = (value + step).min(high);
    let capsule = modifier
        .size(Size::new(WIDTH, HEIGHT))
        .draw_behind(move |scope| {
            scope.draw_round_rect(Brush::solid(fill), CornerRadii::uniform(HEIGHT * 0.5));
            scope.draw_rect_at(
                Rect {
                    x: (WIDTH - RULE_WIDTH) * 0.5,
                    y: RULE_INSET,
                    width: RULE_WIDTH,
                    height: HEIGHT - 2.0 * RULE_INSET,
                },
                Brush::solid(rule),
            );
        });
    let state = format!("{value}");
    Row(capsule, RowSpec::default(), move || {
        let glyph = |enabled: bool| {
            if enabled {
                colors.label
            } else {
                colors.tertiary_label
            }
        };
        Box(
            stepper_half(
                StepperButton {
                    label: cranpose_ui::UiString::Decrement.resolve(),
                    state: state.clone(),
                    target: (value > low).then_some(lower),
                    glyph: Glyph::Minus,
                    color: glyph(value > low),
                },
                Rc::clone(&on_change),
            ),
            BoxSpec::default(),
            || {},
        );
        Box(
            stepper_half(
                StepperButton {
                    label: cranpose_ui::UiString::Increment.resolve(),
                    state: state.clone(),
                    target: (value < high).then_some(higher),
                    glyph: Glyph::Plus,
                    color: glyph(value < high),
                },
                Rc::clone(&on_change),
            ),
            BoxSpec::default(),
            || {},
        );
    });
}

#[derive(Clone, Copy)]
enum Glyph {
    Minus,
    Plus,
}

// One half of the stepper: its reader label and the value it reads with,
// the value it sets, or `None` while the bound disables it.
struct StepperButton {
    label: String,
    state: String,
    target: Option<f32>,
    glyph: Glyph,
    color: Color,
}

// The modifier of one half: its glyph, its click and its reader node.
fn stepper_half(button: StepperButton, on_change: Rc<dyn Fn(f32)>) -> Modifier {
    let StepperButton {
        label,
        state,
        target,
        glyph,
        color,
    } = button;
    let center_x = match glyph {
        Glyph::Minus => MINUS_CENTER_X,
        Glyph::Plus => PLUS_CENTER_X - WIDTH * 0.5,
    };
    let mut half = Modifier::empty()
        .size(Size::new(WIDTH * 0.5, HEIGHT))
        .draw_behind(move |scope| {
            let stroke = Stroke::new(GLYPH_STROKE).with_cap(StrokeCap::Butt);
            let half_length = GLYPH_LENGTH * 0.5;
            scope.draw_line(
                Brush::solid(color),
                Point::new(center_x - half_length, GLYPH_CENTER_Y),
                Point::new(center_x + half_length, GLYPH_CENTER_Y),
                stroke,
            );
            if matches!(glyph, Glyph::Plus) {
                scope.draw_line(
                    Brush::solid(color),
                    Point::new(center_x, GLYPH_CENTER_Y - half_length),
                    Point::new(center_x, GLYPH_CENTER_Y + half_length),
                    stroke,
                );
            }
        });
    if let Some(target) = target {
        half = half.clickable(move |_| on_change(target));
    }
    let enabled = target.is_some();
    half.semantics(move |config: &mut SemanticsConfiguration| {
        config.role = Some(SemanticsWidgetRole::Button);
        config.content_description = Some(label.clone());
        config.state_description = Some(state.clone());
        config.enabled = enabled;
    })
}
