use cranpose_animation::{AnimationSpec, AnimationType, Easing, animateColorAsState};
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, Modifier, Point, SemanticsWidgetRole, Size,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, Rect, Stroke, StrokeCap, StrokeJoin};

use crate::theme::liquid_colors;

/// The drawn box or circle; the control's touch target is the minimum
/// interactive size around it.
const MARK_SIZE: f32 = 22.0;
const BOX_RADIUS: f32 = 6.0;
const OUTLINE_WIDTH: f32 = 2.0;
const CHECK_WIDTH: f32 = 2.5;
const DOT_RADIUS: f32 = 5.0;

fn mark_motion() -> AnimationType {
    AnimationType::Tween(AnimationSpec::tween(160, Easing::EaseOut))
}

/// A check box. `checked` is owned by the caller; `on_change` receives the
/// requested new value. A tap, Space or Enter, and a screen reader's
/// activation all toggle it, and readers announce it as a check box with its
/// state. Name it with a `content_description` on `modifier` or with a label
/// beside it in a merged row.
///
/// Compose's `Checkbox(checked, onCheckedChange)`.
#[composable]
pub fn LiquidCheckbox(modifier: Modifier, checked: bool, on_change: impl Fn(bool) + 'static) {
    let colors = liquid_colors();
    let fill = animateColorAsState(
        if checked {
            colors.accent
        } else {
            Color::TRANSPARENT
        },
        mark_motion(),
        "checkbox-fill",
    )
    .value();
    let outline = if checked {
        colors.accent
    } else {
        colors.secondary_label
    };
    let check = colors.on_accent;
    let mark = Modifier::empty()
        .size(Size::new(MARK_SIZE, MARK_SIZE))
        .draw_behind(move |scope| {
            let bounds = Rect::from_size(scope.size());
            let radii = CornerRadii::uniform(BOX_RADIUS);
            scope.draw_round_rect_at(bounds, Brush::solid(fill), radii);
            let half = OUTLINE_WIDTH * 0.5;
            let outline_rect = Rect {
                x: half,
                y: half,
                width: bounds.width - OUTLINE_WIDTH,
                height: bounds.height - OUTLINE_WIDTH,
            };
            scope.draw_round_rect_at_stroked(
                outline_rect,
                Brush::solid(outline),
                CornerRadii::uniform(BOX_RADIUS - half),
                Stroke::new(OUTLINE_WIDTH),
            );
            if checked {
                draw_check(scope, check);
            }
        });
    let control = modifier.minimum_interactive_component_size().toggleable(
        checked,
        None,
        Some(SemanticsWidgetRole::Checkbox),
        on_change,
    );
    Box(
        control,
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Box(mark.clone(), BoxSpec::default(), || {});
        },
    );
}

/// One option of a group of which one is picked. `selected` is owned by the
/// caller; `on_click` picks this option. Put the buttons of one group in a
/// container with `Modifier::selectable_group`, so a reader says which of how
/// many each one is.
///
/// Compose's `RadioButton(selected, onClick)`.
#[composable]
pub fn LiquidRadioButton(modifier: Modifier, selected: bool, on_click: impl Fn() + 'static) {
    let colors = liquid_colors();
    let outline = animateColorAsState(
        if selected {
            colors.accent
        } else {
            colors.secondary_label
        },
        mark_motion(),
        "radio-outline",
    )
    .value();
    let dot = colors.accent;
    let mark = Modifier::empty()
        .size(Size::new(MARK_SIZE, MARK_SIZE))
        .draw_behind(move |scope| {
            let size = scope.size();
            let center = Point::new(size.width * 0.5, size.height * 0.5);
            scope.draw_circle_stroked(
                Brush::solid(outline),
                center,
                (MARK_SIZE - OUTLINE_WIDTH) * 0.5,
                Stroke::new(OUTLINE_WIDTH),
            );
            if selected {
                scope.draw_circle(Brush::solid(dot), center, DOT_RADIUS);
            }
        });
    let control = modifier.minimum_interactive_component_size().selectable(
        selected,
        Some(SemanticsWidgetRole::RadioButton),
        on_click,
    );
    Box(
        control,
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Box(mark.clone(), BoxSpec::default(), || {});
        },
    );
}

/// The check mark: a short stroke down to the lower left third and a long
/// one up to the upper right corner.
fn draw_check(scope: &mut dyn cranpose_ui_graphics::DrawScope, color: Color) {
    let stroke = Stroke::new(CHECK_WIDTH)
        .with_cap(StrokeCap::Round)
        .with_join(StrokeJoin::Round);
    let at = |x: f32, y: f32| Point::new(MARK_SIZE * x, MARK_SIZE * y);
    let brush = Brush::solid(color);
    scope.draw_line(brush.clone(), at(0.27, 0.52), at(0.43, 0.68), stroke);
    scope.draw_line(brush, at(0.43, 0.68), at(0.74, 0.34), stroke);
}
