// Copyright 2025 The Compose-RS Authors
// SPDX-License-Identifier: Apache-2.0

//! Nearest range state for optimized key→index lookup.
//!
//! Based on JC's `LazyLayoutNearestRangeState`. Uses a sliding window
//! to limit key lookup to items near the current scroll position,
//! providing O(1) lookup instead of O(n).

use std::ops::Range;

/// Sliding window size for key lookup optimization.
/// JC uses 30 for lists, 90 for grids.
pub const NEAREST_ITEMS_SLIDING_WINDOW_SIZE: usize = 30;

/// Extra items to include beyond the sliding window.
/// JC uses 100.
pub const NEAREST_ITEMS_EXTRA_COUNT: usize = 100;

/// Tracks a range of indices near the first visible item for optimized key lookup.
///
/// Instead of searching all items (O(n)), we only search within this range.
/// The range is calculated using a sliding window that only updates when
/// the first visible item crosses a window boundary.
///
/// Matches JC's `LazyLayoutNearestRangeState`.
#[derive(Debug, Clone)]
pub struct NearestRangeState {
    /// Current range of indices to search for keys.
    value: Range<usize>,
    /// Last known first visible item index.
    last_first_visible_item: usize,
    /// Size of the sliding window.
    sliding_window_size: usize,
    /// Extra items to include on each side.
    extra_item_count: usize,
}

impl Default for NearestRangeState {
    fn default() -> Self {
        Self::new(0)
    }
}

impl NearestRangeState {
    /// Creates a new NearestRangeState with default window sizes.
    pub fn new(first_visible_item: usize) -> Self {
        Self::with_sizes(
            first_visible_item,
            NEAREST_ITEMS_SLIDING_WINDOW_SIZE,
            NEAREST_ITEMS_EXTRA_COUNT,
        )
    }

    /// Creates a NearestRangeState with custom window sizes.
    pub fn with_sizes(
        first_visible_item: usize,
        sliding_window_size: usize,
        extra_item_count: usize,
    ) -> Self {
        let value = Self::calculate_range(first_visible_item, sliding_window_size, extra_item_count);
        Self {
            value,
            last_first_visible_item: first_visible_item,
            sliding_window_size,
            extra_item_count,
        }
    }

    /// Returns the current range of indices to search.
    pub fn range(&self) -> Range<usize> {
        self.value.clone()
    }

    /// Updates the range based on the new first visible item.
    /// Only recalculates when crossing a window boundary.
    pub fn update(&mut self, first_visible_item: usize) {
        if first_visible_item != self.last_first_visible_item {
            self.last_first_visible_item = first_visible_item;
            self.value = Self::calculate_range(
                first_visible_item,
                self.sliding_window_size,
                self.extra_item_count,
            );
        }
    }

    /// Calculates the range of items to include.
    /// Optimized to return the same range for small changes in firstVisibleItem.
    fn calculate_range(
        first_visible_item: usize,
        sliding_window_size: usize,
        extra_item_count: usize,
    ) -> Range<usize> {
        let sliding_window_start = sliding_window_size * (first_visible_item / sliding_window_size);
        let start = sliding_window_start.saturating_sub(extra_item_count);
        let end = sliding_window_start + sliding_window_size + extra_item_count;
        start..end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_range() {
        let state = NearestRangeState::new(0);
        // Window starts at 0, so range is 0..(0 + 30 + 100) = 0..130
        assert_eq!(state.range(), 0..130);
    }

    #[test]
    fn test_range_after_small_scroll() {
        let mut state = NearestRangeState::new(0);
        state.update(5);
        // Still in first window (0..30), so range stays 0..130
        assert_eq!(state.range(), 0..130);
    }

    #[test]
    fn test_range_after_crossing_window() {
        let mut state = NearestRangeState::new(0);
        state.update(35);
        // Now in second window (30..60)
        // Start: 30 - 100 = 0 (saturating)
        // End: 30 + 30 + 100 = 160
        assert_eq!(state.range(), 0..160);
    }

    #[test]
    fn test_range_far_scroll() {
        let mut state = NearestRangeState::new(0);
        state.update(1000);
        // Window: 990..1020
        // Start: 990 - 100 = 890
        // End: 990 + 30 + 100 = 1120
        assert_eq!(state.range(), 890..1120);
    }
}
