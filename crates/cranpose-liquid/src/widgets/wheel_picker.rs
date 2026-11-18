use std::{cell::Cell, rc::Rc};

use cranpose_core::{SideEffect, derivedStateOf, remember};
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, Modifier, rememberLazyListStateWithPosition,
    widgets::{Box, BoxSpec},
};

use super::wheel::{PICKER_HEIGHT, Wheel, draw_band, wheel_value};
use crate::theme::liquid_colors;

/// A wheel of `options` that rests with `selected` in the band at its
/// middle. A drag or a fling turns it; once it comes to rest on another
/// option, `on_selected_change` receives that option's index. A new
/// `selected` from the caller turns the wheel to it. Readers hear `label`
/// with the option at the band and can step it like a slider. Size its
/// width with `modifier`; its height is the iOS wheel's 216 points.
///
/// UIKit's `UIPickerView` and SwiftUI's `Picker` with `.wheel`.
#[composable]
pub fn LiquidWheelPicker(
    modifier: Modifier,
    label: &str,
    options: &[&str],
    selected: usize,
    on_selected_change: impl Fn(usize) + 'static,
) {
    let count = options.len();
    let selected = selected.min(count.saturating_sub(1));
    let labels: Rc<[String]> = options.iter().map(|option| (*option).to_owned()).collect();
    let list = rememberLazyListStateWithPosition(selected, 0.0);
    // The caller's last selection, to tell its changes from the wheel's.
    let known = remember(|| Rc::new(Cell::new(selected))).with(Rc::clone);
    let resting =
        derivedStateOf(move || (!list.is_scroll_in_progress()).then(|| wheel_value(list, count)))
            .get();
    SideEffect(move || {
        if known.replace(selected) != selected {
            list.scroll_to_item(selected, 0.0);
        } else if let Some(value) = resting.filter(|value| *value != selected) {
            on_selected_change(value);
        }
    });
    let band = liquid_colors().quaternary_fill;
    let label = label.to_owned();
    Box(
        modifier
            .height(PICKER_HEIGHT)
            .draw_behind(move |scope| draw_band(scope, band)),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Wheel(
                Modifier::empty().fill_max_width(),
                list,
                Rc::clone(&labels),
                Some(label.clone()),
            );
        },
    );
}
