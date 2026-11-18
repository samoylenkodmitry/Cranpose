//! One wheel of the iOS pickers: a column of labels that turns by a drag or
//! a fling and rests with one label in the band at its middle.
//!
//! iOS draws the wheel twice. Rows outside the band sit on a cylinder seen
//! in perspective, in the label color faded toward the edges. Inside the
//! band the same rows lie flat in a larger size and the full label color, so
//! a row crossing the band's edge changes size and color at that edge. Both
//! layers turn together, one row pitch per row.

use std::rc::Rc;

use cranpose_core::{DisposableEffect, derivedStateOf};
use cranpose_macros::composable;
use cranpose_ui::{
    LazyColumn, LazyColumnSpec, LazyItems, LazyListScope, LazyListState, Modifier, PaddingValues,
    ProgressBarRangeInfo, SemanticsSetProgress,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawScope, DrawTextStyle, FontWeight, Point, Rect, Size, TextAlign,
    TextVerticalAlign,
};

use crate::theme::{liquid_colors, liquid_typography};

// The iOS 27 wheel, measured on the simulator at 3x with every row labeled
// alike: rows 32 points apart on a cylinder of radius 93 seen from 1340
// points, in a frame 216 points tall. Outside the band the labels are 21
// points and the label color at 36%, which falls to nothing between 50 and
// 108 points from the middle. Inside a lens 34 points tall they are 23.5
// points and opaque.
pub(super) const ROW: f32 = 32.0;
const FRAME: f32 = 216.0;
const RADIUS: f32 = 93.0;
const CAMERA: f32 = 1340.0;
const LENS_HALF: f32 = 17.0;
const OUTER_SIZE: f32 = 21.0;
const LENS_SIZE: f32 = 23.5;
const OUTER_ALPHA: f32 = 0.357;
const FADE_START: f32 = 50.0;
const FADE_END: f32 = 108.0;
// The band behind the lens: 30 points tall with 13 point corners, 9 points
// in from the picker's sides, in the quaternary fill.
const BAND_HEIGHT: f32 = 30.0;
const BAND_CORNER: f32 = 13.0;
const BAND_INSET: f32 = 9.0;
// The flat list's padding above its first row and below its last, so
// either can rest in the middle and its neighbours fill the rest.
const END_PADDING: f32 = (FRAME - ROW) * 0.5;

// The value a wheel's list rests on: the row in its middle.
pub(super) fn wheel_value(list: LazyListState, count: usize) -> usize {
    let index = list.first_visible_item_index();
    let past_half = list.first_visible_item_scroll_offset() > ROW / 2.0;
    (index + usize::from(past_half)).min(count.saturating_sub(1))
}

// Where a row `offset` points below the middle of the flat list lands on the
// cylinder: its middle, how much it shrinks across and down, and its alpha.
#[derive(Clone, Copy)]
struct Projection {
    center: f32,
    scale_x: f32,
    scale_y: f32,
    alpha: f32,
}

fn project(offset: f32) -> Option<Projection> {
    let angle = offset / RADIUS;
    if angle.abs() >= std::f32::consts::FRAC_PI_2 {
        return None;
    }
    let (sin, cos) = angle.sin_cos();
    let perspective = CAMERA / (CAMERA + RADIUS * (1.0 - cos));
    let center = RADIUS * sin * perspective;
    let fade = ((FADE_END - center.abs()) / (FADE_END - FADE_START)).clamp(0.0, 1.0);
    Some(Projection {
        center,
        scale_x: perspective,
        scale_y: cos * perspective,
        alpha: OUTER_ALPHA * fade,
    })
}

// Draws the band in `fill` across a picker of the scope's size, behind its
// wheels.
pub(super) fn draw_band(scope: &mut dyn DrawScope, fill: Color) {
    let size = scope.size();
    scope.draw_round_rect_at(
        Rect {
            x: BAND_INSET,
            y: (size.height - BAND_HEIGHT) * 0.5,
            width: (size.width - 2.0 * BAND_INSET).max(0.0),
            height: BAND_HEIGHT,
        },
        Brush::solid(fill),
        CornerRadii::uniform(BAND_CORNER),
    );
}

// The height of a picker: the height of its wheels.
pub(super) const PICKER_HEIGHT: f32 = FRAME;

// The wheel's text at `size` points, centered on its row.
fn wheel_text(size: f32) -> DrawTextStyle {
    // The body weight, which bold text raises.
    let weight = liquid_typography()
        .body
        .span_style
        .font_weight
        .map_or(FontWeight::NORMAL, |weight| FontWeight(weight.0));
    DrawTextStyle {
        font_weight: weight,
        align: TextAlign::Center,
        vertical_align: TextVerticalAlign::Center,
        ..DrawTextStyle::new(size)
    }
}

// The labels a wheel shows, compared by identity.
#[derive(Clone)]
struct Labels(Rc<[String]>);

impl PartialEq for Labels {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// One wheel, as wide as `modifier` makes it. The picker that holds it draws
// the band. A flat list of empty rows takes the drags and flings; the wheel
// draws every label itself, so a turn repaints the wheel and composes
// nothing.
#[composable]
pub(super) fn Wheel(
    modifier: Modifier,
    list: LazyListState,
    labels: Rc<[String]>,
    name: Option<String>,
) {
    DisposableEffect((), move |scope| {
        // A release rests on the nearer row.
        list.set_settle_policy(Some(Rc::new(
            |proposed, _velocity| {
                if proposed > ROW / 2.0 { ROW } else { 0.0 }
            },
        )));
        scope.on_dispose(move || list.set_settle_policy(None))
    });
    let count = labels.len();
    // Composition reads the value only as it changes rows, never per frame.
    let value = derivedStateOf(move || wheel_value(list, count)).get();
    let current = labels.get(value).cloned().unwrap_or_default();
    let color = liquid_colors().label;
    let semantics = move |config: &mut cranpose_ui::SemanticsConfiguration| {
        config.content_description.clone_from(&name);
        config.state_description = Some(current.clone());
        config.progress = Some(ProgressBarRangeInfo::new(
            value as f32,
            0.0,
            count.saturating_sub(1) as f32,
            count.saturating_sub(2) as u32,
        ));
        config.set_progress = Some(SemanticsSetProgress::new(move |next: f32| {
            let row = next.round().clamp(0.0, count.saturating_sub(1) as f32) as usize;
            list.scroll_to_item(row, 0.0);
            true
        }));
    };
    let drawn = Labels(labels);
    let (outer, lens) = (wheel_text(OUTER_SIZE), wheel_text(LENS_SIZE));
    Box(
        modifier
            .height(FRAME)
            .clip_to_bounds()
            .draw_behind(move |scope| draw_wheel(scope, list, &drawn.0, color, &outer, &lens))
            .value_picker()
            .semantics(semantics),
        BoxSpec::default(),
        move || {
            LazyColumn(
                Modifier::empty().fill_max_size(),
                list,
                LazyColumnSpec::default()
                    .content_padding(PaddingValues::symmetric(0.0, END_PADDING)),
                move |scope| {
                    scope.items(LazyItems::new(count), |_| {
                        Box(
                            Modifier::empty().fill_max_width().height(ROW),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                },
            );
        },
    );
}

// Rows that can show on each side of the middle: those within a quarter turn
// of the cylinder, 93 * pi / 2 points of the flat list.
const REACH: usize = 5;

// Draws the labels: on the cylinder above and below the lens, flat and larger
// inside it.
fn draw_wheel(
    scope: &mut dyn DrawScope,
    list: LazyListState,
    labels: &[String],
    color: Color,
    outer: &DrawTextStyle,
    lens: &DrawTextStyle,
) {
    let size = scope.size();
    let middle = size.height * 0.5;
    let first = list.first_visible_item_index();
    let scrolled = list.first_visible_item_scroll_offset();
    // How far a row sits below the middle, in points of the flat list. The
    // row resting in the middle is the list's first visible row.
    let offset = |row: usize| (row as f32 - first as f32) * ROW - scrolled;
    let near = |reach: usize| {
        let start = first.saturating_sub(reach);
        labels
            .iter()
            .enumerate()
            .skip(start)
            .take(first + reach + 1 - start)
    };
    let lens_top = middle - LENS_HALF;
    let lens_bottom = middle + LENS_HALF;
    for clip in [
        Rect::from_origin_size(Point::new(0.0, 0.0), Size::new(size.width, lens_top)),
        Rect::from_origin_size(
            Point::new(0.0, lens_bottom),
            Size::new(size.width, size.height - lens_bottom),
        ),
    ] {
        scope.clip_rect(clip, &mut |scope| {
            for (row, label) in near(REACH) {
                let Some(projection) = project(offset(row)) else {
                    continue;
                };
                let center = Point::new(size.width * 0.5, middle + projection.center);
                let brush = Brush::solid(color.with_alpha(color.a() * projection.alpha));
                scope.scale(
                    projection.scale_x,
                    projection.scale_y,
                    center,
                    &mut |scope| draw_row(scope, center.y, brush.clone(), label, outer),
                );
            }
        });
    }
    let lens_rect = Rect::from_origin_size(
        Point::new(0.0, lens_top),
        Size::new(size.width, lens_bottom - lens_top),
    );
    scope.clip_rect(lens_rect, &mut |scope| {
        for (row, label) in near(1) {
            draw_row(
                scope,
                middle + offset(row),
                Brush::solid(color),
                label,
                lens,
            );
        }
    });
}

// Draws one label centered on `center_y`, across the scope.
fn draw_row(
    scope: &mut dyn DrawScope,
    center_y: f32,
    brush: Brush,
    label: &str,
    style: &DrawTextStyle,
) {
    let width = scope.size().width;
    scope.draw_text_at(
        Rect::from_origin_size(Point::new(0.0, center_y - ROW), Size::new(width, 2.0 * ROW)),
        brush,
        label,
        style,
    );
}
