use std::rc::Rc;

use cranpose_core::rememberMutableStateOf;
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, SemanticsConfiguration, SemanticsWidgetRole, Size,
    widgets::{Box, BoxSpec, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, Color, DrawScope, Point, Stroke, StrokeCap, StrokeJoin};
use cranpose_ui_layout::VerticalAlignment;

use super::menu::{LiquidDropdownMenu, LiquidDropdownMenuSpec, LiquidMenuItem};
use crate::theme::{liquid_colors, liquid_typography};

// The iOS 27 pop-up button, measured on the simulator at 3x: the option in
// the accent body text, then 19/3 points on, an up chevron over a down
// chevron in a box 26/3 points wide and 38/3 tall. Each chevron's ends sit 11/3
// points either side of its middle and 1.75 points from the box's middle,
// its tip 5.6 points from it, and its stroke is 4/3 points.
const GAP: f32 = 19.0 / 3.0;
const CHEVRONS_WIDTH: f32 = 26.0 / 3.0;
const CHEVRONS_HEIGHT: f32 = 38.0 / 3.0;
const HALF_SPAN: f32 = 11.0 / 3.0;
const END_Y: f32 = 1.75;
const TIP_Y: f32 = 5.6;
const STROKE: f32 = 4.0 / 3.0;

/// The selected option of `options` in the accent color beside an up and
/// down chevron. A tap opens a menu of the options with a check at the
/// selected one; choosing one passes its index to `on_selected_change` and
/// closes the menu. Readers hear `label` as a dropdown list with the option.
///
/// UIKit's `UIButton` with a menu and `showsMenuAsPrimaryAction`, and
/// SwiftUI's `Picker` with `.menu`.
#[composable]
pub fn LiquidPopUpButton(
    modifier: Modifier,
    label: &str,
    options: &[&str],
    selected: usize,
    on_selected_change: impl Fn(usize) + 'static,
) {
    let colors = liquid_colors();
    let mut style = liquid_typography().body;
    style.span_style.color = Some(colors.accent);
    let options: Rc<[String]> = options.iter().map(|option| (*option).to_owned()).collect();
    let current = options.get(selected).cloned().unwrap_or_default();
    let expanded = rememberMutableStateOf(|| false);
    let on_selected_change: Rc<dyn Fn(usize)> = Rc::new(on_selected_change);
    let label = label.to_owned();
    let accent = colors.accent;
    LiquidDropdownMenu(
        modifier,
        expanded.get(),
        LiquidDropdownMenuSpec::default(),
        move || expanded.set(false),
        move || {
            let reader_state = current.clone();
            let reader_label = label.clone();
            Row(
                Modifier::empty()
                    .clickable(move |_| expanded.set(true))
                    .semantics(move |config: &mut SemanticsConfiguration| {
                        config.role = Some(SemanticsWidgetRole::DropdownList);
                        config.content_description = Some(reader_label.clone());
                        config.state_description = Some(reader_state.clone());
                    }),
                RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
                {
                    let current = current.clone();
                    let style = style.clone();
                    move || {
                        Text(
                            current.clone(),
                            Modifier::empty().hide_from_accessibility(),
                            style.clone(),
                        );
                        Box(
                            Modifier::empty()
                                .padding_each(GAP, 0.0, 0.0, 0.0)
                                .size(Size::new(CHEVRONS_WIDTH, CHEVRONS_HEIGHT))
                                .draw_behind(move |scope| draw_chevrons(scope, accent)),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        },
        move |scope| {
            for (index, option) in options.iter().enumerate() {
                let on_selected_change = Rc::clone(&on_selected_change);
                scope.item(
                    LiquidMenuItem::new(option.clone()).checked(index == selected),
                    move || on_selected_change(index),
                );
            }
        },
    );
}

// The up chevron over the down chevron, centered in the scope.
fn draw_chevrons(scope: &mut dyn DrawScope, color: Color) {
    let size = scope.size();
    let (x, y) = (size.width * 0.5, size.height * 0.5);
    let stroke = Stroke::new(STROKE)
        .with_cap(StrokeCap::Round)
        .with_join(StrokeJoin::Round);
    for direction in [-1.0, 1.0] {
        let tip = Point::new(x, y + direction * TIP_Y);
        for side in [-1.0, 1.0] {
            scope.draw_line(
                Brush::solid(color),
                Point::new(x + side * HALF_SPAN, y + direction * END_Y),
                tip,
                stroke,
            );
        }
    }
}
