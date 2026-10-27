// The scroll position of a lazy list: its first visible item and how far
// that item is scrolled.

use std::cell::RefCell;

use cranpose_core::{MutableState, RuntimeCell};

use crate::lazy::nearest_range::NearestRangeState;

/// Contains the current scroll position represented by the first visible item
/// index and the first visible item scroll offset.
///
/// This is a `Copy` type that holds reactive state. Reading `index` or `scroll_offset`
/// during composition creates a snapshot dependency for automatic recomposition.
///
/// Matches Jetpack Compose's `LazyListScrollPosition` design.
#[derive(Clone, Copy)]
pub struct LazyListScrollPosition {
    pub(super) index: MutableState<usize>,
    pub(super) scroll_offset: MutableState<f32>,
    inner: RuntimeCell<RefCell<ScrollPositionInner>>,
}

/// Non-reactive internal state for scroll position.
struct ScrollPositionInner {
    current_index: usize,
    current_scroll_offset: f32,
    last_known_first_item_key: Option<u64>,
    nearest_range_state: NearestRangeState,
}

impl LazyListScrollPosition {
    pub(super) fn new(index: usize, scroll_offset: f32) -> Self {
        Self {
            index: cranpose_core::mutableStateOf(index),
            scroll_offset: cranpose_core::mutableStateOf(scroll_offset),
            inner: RuntimeCell::new(RefCell::new(ScrollPositionInner {
                current_index: index,
                current_scroll_offset: scroll_offset,
                last_known_first_item_key: None,
                nearest_range_state: NearestRangeState::new(index),
            })),
        }
    }

    // The first visible item and how far it is scrolled, or `None` once the
    // slot that held the state was released.
    pub(super) fn current(&self) -> Option<(usize, f32)> {
        self.inner.try_with(|inner| {
            let inner = inner.borrow();
            (inner.current_index, inner.current_scroll_offset)
        })
    }

    pub(super) fn current_index(&self) -> usize {
        self.inner
            .try_with(|cell| cell.borrow().current_index)
            .unwrap_or(0)
    }

    pub(super) fn current_scroll_offset(&self) -> f32 {
        self.inner
            .try_with(|cell| cell.borrow().current_scroll_offset)
            .unwrap_or(0.0)
    }

    /// Returns the index of the first visible item (reactive read).
    pub fn index(&self) -> usize {
        if !self.index.is_alive() {
            return 0;
        }
        let Some(index) = self.inner.try_with(|cell| cell.borrow().current_index) else {
            return 0;
        };
        self.index.subscribe_current_scope_only();
        index
    }

    /// Returns the scroll offset of the first visible item (reactive read).
    pub fn scroll_offset(&self) -> f32 {
        if !self.scroll_offset.is_alive() {
            return 0.0;
        }
        let Some(offset) = self
            .inner
            .try_with(|cell| cell.borrow().current_scroll_offset)
        else {
            return 0.0;
        };
        self.scroll_offset.subscribe_current_scope_only();
        offset
    }

    pub(crate) fn update_from_measure_result(
        &self,
        first_visible_index: usize,
        first_visible_scroll_offset: f32,
        first_visible_item_key: Option<u64>,
    ) {
        if !self.index.is_alive() || !self.scroll_offset.is_alive() {
            return;
        }
        let updated = self.inner.try_with(|cell| {
            let mut inner = cell.borrow_mut();
            inner.current_index = first_visible_index;
            inner.current_scroll_offset = first_visible_scroll_offset;
            inner.last_known_first_item_key = first_visible_item_key;
            inner.nearest_range_state.update(first_visible_index);
        });
        if updated.is_none() {
            return;
        }

        if self.index.get_non_reactive() != first_visible_index {
            self.index.set(first_visible_index);
        }
        if (self.scroll_offset.get_non_reactive() - first_visible_scroll_offset).abs() > 0.001 {
            self.scroll_offset.set(first_visible_scroll_offset);
        }
    }

    pub(crate) fn request_position_and_forget_last_known_key(
        &self,
        index: usize,
        scroll_offset: f32,
    ) {
        if !self.index.is_alive() || !self.scroll_offset.is_alive() {
            return;
        }
        let updated = self.inner.try_with(|cell| {
            let mut inner = cell.borrow_mut();
            inner.current_index = index;
            inner.current_scroll_offset = scroll_offset;
            inner.last_known_first_item_key = None;
            inner.nearest_range_state.update(index);
        });
        if updated.is_none() {
            return;
        }

        if self.index.get_non_reactive() != index {
            self.index.set(index);
        }
        if (self.scroll_offset.get_non_reactive() - scroll_offset).abs() > 0.001 {
            self.scroll_offset.set(scroll_offset);
        }
    }

    pub(crate) fn update_if_first_item_moved<F>(
        &self,
        new_item_count: usize,
        find_by_key: F,
    ) -> usize
    where
        F: Fn(u64) -> Option<usize>,
    {
        let Some((current_index, new_index)) = self.first_item_index(new_item_count, find_by_key)
        else {
            return 0;
        };
        if current_index != new_index
            && let Some(cell) = self.inner.try_get()
        {
            {
                let mut inner = cell.borrow_mut();
                inner.current_index = new_index;
                inner.nearest_range_state.update(new_index);
            }
            self.index.set(new_index);
        }
        new_index
    }

    // Whether `update_if_first_item_moved` would move the position.
    pub(crate) fn first_item_moved<F>(&self, new_item_count: usize, find_by_key: F) -> bool
    where
        F: Fn(u64) -> Option<usize>,
    {
        self.first_item_index(new_item_count, find_by_key)
            .is_some_and(|(current_index, new_index)| current_index != new_index)
    }

    // The index of the first visible item and the one it has among
    // `new_item_count` items, found by its key.
    fn first_item_index<F>(&self, new_item_count: usize, find_by_key: F) -> Option<(usize, usize)>
    where
        F: Fn(u64) -> Option<usize>,
    {
        if !self.index.is_alive() {
            return None;
        }
        let (current_index, last_key) = self.inner.try_with(|cell| {
            let inner = cell.borrow();
            (inner.current_index, inner.last_known_first_item_key)
        })?;
        let new_index = match last_key {
            None => current_index.min(new_item_count.saturating_sub(1)),
            Some(key) => find_by_key(key)
                .unwrap_or_else(|| current_index.min(new_item_count.saturating_sub(1))),
        };
        Some((current_index, new_index))
    }

    /// Returns the nearest range for optimized key lookups.
    pub fn nearest_range(&self) -> std::ops::Range<usize> {
        self.inner
            .try_with(|cell| cell.borrow().nearest_range_state.range())
            .unwrap_or(0..0)
    }
}
