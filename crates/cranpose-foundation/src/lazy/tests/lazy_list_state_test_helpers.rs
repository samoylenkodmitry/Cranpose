use std::sync::Arc;

use cranpose_core::{DefaultScheduler, Runtime};

use super::*;

pub fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

pub fn new_lazy_list_state() -> LazyListState {
    new_lazy_list_state_with_position(0, 0.0)
}

// forwards on purpose: the name the lazy-list tests call it by, in about
// fifty places; the state itself is built by `LazyListState::new`.
pub fn new_lazy_list_state_with_position(
    initial_first_visible_item_index: usize,
    initial_first_visible_item_scroll_offset: f32,
) -> LazyListState {
    LazyListState::new(
        initial_first_visible_item_index,
        initial_first_visible_item_scroll_offset,
    )
}
