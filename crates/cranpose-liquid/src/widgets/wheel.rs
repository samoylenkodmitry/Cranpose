//! One wheel of the iOS pickers: a column of labels that turns by a drag or
//! a fling and rests with one label in the band at its middle.

use std::rc::Rc;

use cranpose_core::DisposableEffect;
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, LazyColumn, LazyColumnSpec, LazyItems, LazyListScope, LazyListState, Modifier,
    ProgressBarRangeInfo, SemanticsSetProgress, Size, TextStyle,
    widgets::{Box, BoxSpec, Text},
};
use cranpose_ui_graphics::{Brush, CornerRadii, Point, Rect};

use crate::theme::liquid_colors;

// The height of one row of a wheel.
pub(super) const ROW: f32 = 40.0;
// Rows a wheel shows: the picked one in the middle and two on each side.
const VISIBLE_ROWS: usize = 5;
// Empty rows before the first value and after the last, so both can sit
// in the middle.
const PAD: usize = VISIBLE_ROWS / 2;

// The value a wheel's list rests on: the row in its middle.
pub(super) fn wheel_value(list: LazyListState, count: usize) -> usize {
    let index = list.first_visible_item_index();
    let past_half = list.first_visible_item_scroll_offset() > ROW / 2.0;
    (index + usize::from(past_half)).min(count.saturating_sub(1))
}

// The labels a wheel shows, compared by identity.
#[derive(Clone)]
struct Labels(Rc<[String]>);

impl PartialEq for Labels {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[composable]
pub(super) fn Wheel(
    list: LazyListState,
    labels: Rc<[String]>,
    name: Option<String>,
    width: f32,
    style: TextStyle,
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
    let value = wheel_value(list, count);
    let current = labels.get(value).cloned().unwrap_or_default();
    let band = liquid_colors().fill;
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
    let labels = Labels(labels);
    Box(
        Modifier::empty()
            .size(Size::new(width, ROW * VISIBLE_ROWS as f32))
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect_at(
                    Rect::from_origin_size(
                        Point::new(0.0, ROW * PAD as f32),
                        Size::new(size.width, ROW),
                    ),
                    Brush::solid(band),
                    CornerRadii::uniform(8.0),
                );
            })
            .value_picker()
            .semantics(semantics),
        BoxSpec::default(),
        move || {
            let labels = labels.clone();
            let style = style.clone();
            LazyColumn(
                Modifier::empty().fill_max_size(),
                list,
                LazyColumnSpec::default(),
                move |scope| {
                    let labels = labels.clone();
                    let style = style.clone();
                    scope.items(LazyItems::new(count + 2 * PAD), move |row| {
                        let label = row
                            .checked_sub(PAD)
                            .and_then(|value| labels.0.get(value))
                            .cloned();
                        let style = style.clone();
                        Box(
                            Modifier::empty().fill_max_width().height(ROW),
                            BoxSpec::default().content_alignment(Alignment::CENTER),
                            move || {
                                if let Some(label) = label.clone() {
                                    Text(
                                        label,
                                        Modifier::empty().hide_from_accessibility(),
                                        style.clone(),
                                    );
                                }
                            },
                        );
                    });
                },
            );
        },
    );
}
