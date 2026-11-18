use std::rc::Rc;

use cranpose_animation::{animateFloatAsState, spring};
use cranpose_macros::composable;
use cranpose_ui::{
    GraphicsLayer, Modifier, Point, SemanticsConfiguration, SemanticsExpand, SemanticsWidgetRole,
    Size,
    widgets::{
        AnimatedVisibility, Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text,
        expand_vertically, fade_in, fade_out, shrink_vertically,
    },
};
use cranpose_ui_graphics::{Brush, Color, DrawScope, Stroke, StrokeCap, StrokeJoin};
use cranpose_ui_layout::VerticalAlignment;

use crate::theme::{liquid_colors, liquid_typography};

// The iOS 27 disclosure chevron, measured on the simulator at 3x: a right
// chevron 7 points wide and 12 tall, 2/3 points from the row's trailing
// edge, which turns a quarter turn down as the group opens. It turns about
// the center of a frame 32/3 points wide at that edge, so the open chevron
// sits 7/6 points lower and further leading than the closed one. Its stroke
// width is fitted to the measured ink.
const CHEVRON_WIDTH: f32 = 7.0;
const CHEVRON_HEIGHT: f32 = 12.0;
const CHEVRON_STROKE: f32 = 1.87;
const CHEVRON_TRAILING_INSET: f32 = 2.0 / 3.0;
const CHEVRON_FRAME_WIDTH: f32 = 32.0 / 3.0;
// The label row is at least 27 points tall, and the content's first line
// starts 1 point below it.
const ROW_MIN_HEIGHT: f32 = 27.0;
const CONTENT_GAP: f32 = 1.0;

/// A label in the accent color with a chevron at the trailing edge that
/// shows or hides `content` below it. `expanded` is owned by the caller; a
/// tap on the label row asks `on_expanded_change` for the other state. The
/// chevron turns and the content slides open on the iOS spring. Readers
/// hear the label as a button that is expanded or collapsed.
///
/// SwiftUI's `DisclosureGroup(label, isExpanded:)`.
#[composable]
pub fn LiquidDisclosureGroup(
    modifier: Modifier,
    label: &str,
    expanded: bool,
    on_expanded_change: impl Fn(bool) + 'static,
    content: impl Fn() + 'static,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let mut label_style = typography.body;
    label_style.span_style.color = Some(colors.accent);
    let turn = animateFloatAsState(
        f32::from(expanded),
        spring(1.0, 300.0),
        "liquid-disclosure-chevron",
    );
    let chevron_color = colors.label;
    let on_expanded_change: Rc<dyn Fn(bool)> = Rc::new(on_expanded_change);
    let label = label.to_owned();
    let content: Rc<dyn Fn()> = Rc::new(content);
    Column(modifier, ColumnSpec::default(), move || {
        let toggle = Rc::clone(&on_expanded_change);
        let reader = Rc::clone(&on_expanded_change);
        let label_text = label.clone();
        let label_style = label_style.clone();
        Row(
            Modifier::empty()
                .fill_max_width()
                .height_in(ROW_MIN_HEIGHT, f32::INFINITY)
                .clickable(move |_| toggle(!expanded))
                .semantics(move |config: &mut SemanticsConfiguration| {
                    config.role = Some(SemanticsWidgetRole::Button);
                    let reader = Rc::clone(&reader);
                    let action = SemanticsExpand::new(move || {
                        reader(!expanded);
                        true
                    });
                    if expanded {
                        config.collapse = Some(action);
                    } else {
                        config.expand = Some(action);
                    }
                }),
            RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
            move || {
                Text(
                    label_text.clone(),
                    Modifier::empty().weight(1.0),
                    label_style.clone(),
                );
                Box(
                    Modifier::empty()
                        .size(Size::new(CHEVRON_FRAME_WIDTH, CHEVRON_HEIGHT))
                        .graphics_layer(move || GraphicsLayer {
                            rotation_z: 90.0 * turn.value(),
                            ..Default::default()
                        })
                        .draw_behind(move |scope| draw_chevron(scope, chevron_color)),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
        let motion = spring(1.0, 300.0);
        AnimatedVisibility(
            expanded,
            (expand_vertically() + fade_in()).with_animation(motion),
            (shrink_vertically() + fade_out()).with_animation(motion),
            {
                let content = Rc::clone(&content);
                move || {
                    let content = Rc::clone(&content);
                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .padding_each(0.0, CONTENT_GAP, 0.0, 0.0),
                        BoxSpec::default(),
                        move || content(),
                    );
                }
            },
        );
    });
}

// A right chevron at the trailing edge of the scope, centered vertically.
fn draw_chevron(scope: &mut dyn DrawScope, color: Color) {
    let size = scope.size();
    let right = size.width - CHEVRON_TRAILING_INSET - CHEVRON_STROKE * 0.5;
    let left = right - CHEVRON_WIDTH + CHEVRON_STROKE;
    let top = (size.height - CHEVRON_HEIGHT) * 0.5 + CHEVRON_STROKE * 0.5;
    let bottom = (size.height + CHEVRON_HEIGHT) * 0.5 - CHEVRON_STROKE * 0.5;
    let stroke = Stroke::new(CHEVRON_STROKE)
        .with_cap(StrokeCap::Round)
        .with_join(StrokeJoin::Round);
    let tip = Point::new(right, size.height * 0.5);
    scope.draw_line(Brush::solid(color), Point::new(left, top), tip, stroke);
    scope.draw_line(Brush::solid(color), tip, Point::new(left, bottom), stroke);
}
