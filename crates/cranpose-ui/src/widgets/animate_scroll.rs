//! Animated scrolling of lazy lists, and of the grids and pagers built on
//! them: Compose's `animateScrollBy` and `animateScrollToItem`.

use std::rc::Rc;

use cranpose_foundation::lazy::LazyListState;

use crate::fling_animation::{SettleAnimation, SpringParams};

/// Scrolls a lazy list with a spring. A press on the list, or another
/// animation, stops it where it is.
pub trait AnimateScroll {
    /// Scrolls `distance` logical pixels, positive toward the end.
    fn animate_scroll_by(&self, distance: f32);

    /// Scrolls until item `index` starts `offset` past the start of the
    /// viewport, as [`LazyListState::scroll_to_item`] would place it.
    fn animate_scroll_to_item(&self, index: usize, offset: f32);
}

impl AnimateScroll for LazyListState {
    fn animate_scroll_by(&self, distance: f32) {
        animate(*self, distance, None);
    }

    fn animate_scroll_to_item(&self, index: usize, offset: f32) {
        animate(
            *self,
            distance_to_item(self, index) + offset,
            Some((index, offset)),
        );
    }
}

/// How far item `index` starts past where `scroll_to_item(index, 0)` puts
/// it: measured when the list shows it, estimated from the average item size
/// when not. The animation lands on the item exactly either way.
fn distance_to_item(state: &LazyListState, index: usize) -> f32 {
    let shown = state.with_layout_info(|info| {
        let item = info
            .visible_items_info
            .iter()
            .find(|item| item.index == index)?;
        let leading = if index == 0 {
            info.before_content_padding
        } else {
            0.0
        };
        Some(item.offset - leading)
    });
    if let Some(distance) = shown.flatten() {
        return distance;
    }
    let first = state.first_visible_item_index_non_reactive();
    let scrolled = state.first_visible_item_scroll_offset_non_reactive();
    (index as f32 - first as f32) * state.average_item_size() - scrolled
}

/// Springs `state` through `distance`, then puts it at `land` when the
/// animation ends on its own.
fn animate(state: LazyListState, distance: f32, land: Option<(usize, f32)>) {
    let Some(runtime) = cranpose_core::current_runtime_handle() else {
        match land {
            Some((index, offset)) => state.scroll_to_item(index, offset),
            None => {
                state.dispatch_scroll_delta(-distance);
            }
        }
        return;
    };
    let settle = SettleAnimation::new(runtime, SpringParams::SETTLE_POLICY);
    let stop = settle.clone();
    let id = state.start_scroll_animation(Rc::new(move || stop.cancel()));
    settle.start_settle(
        0.0,
        0.0,
        distance,
        move |delta| -state.dispatch_scroll_delta(-delta),
        move |_| {
            state.end_scroll_animation(id);
            if let Some((index, offset)) = land {
                state.scroll_to_item(index, offset);
            }
        },
    );
}
