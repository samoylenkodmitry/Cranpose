//! Animated scrolling of lazy lists, and of the grids and pagers built on
//! them, and of scrolling columns: Compose's `animateScrollBy`,
//! `animateScrollToItem` and `ScrollState.animateScrollTo`.

use cranpose_foundation::{ScrollId, lazy::LazyListState};

use crate::{
    fling_animation::{SettleAnimation, SpringParams},
    scroll::ScrollState,
};

/// Scrolls a lazy list with a spring. A press on the list, or another
/// animation, stops it where it is.
pub trait AnimateScroll {
    /// Scrolls `distance` logical pixels, positive toward the end.
    fn animate_scroll_by(&self, distance: f32);

    /// Scrolls until item `index` starts `offset` past the start of the
    /// viewport, as [`LazyListState::scroll_to_item`] would place it.
    fn animate_scroll_to_item(&self, index: usize, offset: f32);
}

// A scroll state that a spring moves. It keeps one running scroll, which the
// spring takes.
trait SpringTarget: Copy + 'static {
    // Where a spring that ends on its own puts the state, so that it rests
    // exactly there.
    type Landing: 'static;

    // Moves the content by `delta` toward the end. Returns how far it moved.
    fn move_by(self, delta: f32) -> f32;

    fn land(self, landing: Self::Landing);

    // `None` once the slot that held the state was released.
    fn start_run(self, stop: impl FnOnce() + 'static) -> Option<ScrollId>;

    fn end_run(self, id: ScrollId);
}

impl SpringTarget for LazyListState {
    type Landing = (usize, f32);

    fn move_by(self, delta: f32) -> f32 {
        -self.dispatch_scroll_delta(-delta)
    }

    fn land(self, (index, offset): (usize, f32)) {
        self.scroll_to_item(index, offset);
    }

    fn start_run(self, stop: impl FnOnce() + 'static) -> Option<ScrollId> {
        self.start_scroll_animation(stop)
    }

    fn end_run(self, id: ScrollId) {
        self.end_scroll_animation(id);
    }
}

impl SpringTarget for ScrollState {
    type Landing = f32;

    fn move_by(self, delta: f32) -> f32 {
        self.dispatch_raw_delta(delta)
    }

    fn land(self, value: f32) {
        self.scroll_to(value);
    }

    fn start_run(self, stop: impl FnOnce() + 'static) -> Option<ScrollId> {
        self.start_scroll_animation(stop)
    }

    fn end_run(self, id: ScrollId) {
        self.end_scroll_animation(id);
    }
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

// Springs `state` to `value`, which lies within its range.
pub(crate) fn animate_scroll_state_to(state: ScrollState, value: f32) {
    let Some(current) = state.try_value() else {
        return;
    };
    animate(state, value - current, Some(value));
}

/// How far item `index` starts past where `scroll_to_item(index, 0)` puts
/// it: measured when the list shows it, estimated from the average item size
/// when not. The animation lands on the item exactly either way.
fn distance_to_item(state: &LazyListState, index: usize) -> f32 {
    let shown = state.with_layout_info(|info| {
        info.visible_items_info
            .iter()
            .find(|item| item.index == index)
            .map(|item| item.offset)
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
fn animate<S: SpringTarget>(state: S, distance: f32, land: Option<S::Landing>) {
    let Some(runtime) = cranpose_core::current_runtime_handle() else {
        match land {
            Some(landing) => state.land(landing),
            None => {
                state.move_by(distance);
            }
        }
        return;
    };
    let settle = SettleAnimation::new(runtime, SpringParams::SETTLE_POLICY);
    let stop = settle.clone();
    let id = state.start_run(move || stop.cancel());
    settle.start_settle(
        0.0,
        0.0,
        distance,
        move |delta| state.move_by(delta),
        move |_| {
            if let Some(id) = id {
                state.end_run(id);
            }
            if let Some(landing) = land {
                state.land(landing);
            }
        },
    );
}
