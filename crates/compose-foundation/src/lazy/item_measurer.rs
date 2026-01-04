//! Item measurement for lazy list.
//!
//! This module handles measuring visible items and beyond-bounds buffer items.

use super::lazy_list_measure::LazyListMeasureConfig;
use super::lazy_list_measured_item::LazyListMeasuredItem;
use std::collections::VecDeque;

/// Maximum items to measure per pass as a safety limit.
///
/// This prevents infinite loops when items have zero or near-zero size.
/// With 500 items at typical 12px minimum item height, this supports
/// viewports up to ~6000px. Larger viewports with tiny items may be
/// under-filled, but this is rare in practice.
///
/// If the limit is reached, a warning is logged but measurement continues
/// correctly - the viewport will simply have fewer items than it could fit.
/// Default time budget for a single measurement pass (50ms).
///
/// This provides a safety mechanism to prevent infinite loops or excessive
/// computation time when measuring items, while adapting to device performance
/// better than a hard item count limit.
const DEFAULT_TIME_BUDGET: std::time::Duration = std::time::Duration::from_millis(50);

/// Maximum items to measure per pass as a hard safety limit.
/// Used in addition to time budget to prevent memory exhaustion in extreme cases.
const MAX_VISIBLE_ITEMS_SAFETY: usize = 10000;

/// Measures items for lazy list layout.
///
/// Handles measuring:
/// - Visible items that fill the viewport
/// - Beyond-bounds items after visible (for prefetch)
/// - Beyond-bounds items before visible (for prefetch)
pub struct ItemMeasurer<'a, F> {
    measure_fn: &'a mut F,
    /// Items already measured during scroll normalization to avoid re-composition.
    pre_measured: VecDeque<LazyListMeasuredItem>,
    config: &'a LazyListMeasureConfig,
    items_count: usize,
    effective_viewport_size: f32,
}

impl<'a, F> ItemMeasurer<'a, F>
where
    F: FnMut(usize) -> LazyListMeasuredItem,
{
    /// Creates a new ItemMeasurer.
    pub fn new(
        measure_fn: &'a mut F,
        config: &'a LazyListMeasureConfig,
        items_count: usize,
        effective_viewport_size: f32,
        pre_measured: VecDeque<LazyListMeasuredItem>,
    ) -> Self {
        Self {
            measure_fn,
            pre_measured,
            config,
            items_count,
            effective_viewport_size,
        }
    }

    /// Measures all items: visible + beyond-bounds buffer.
    ///
    /// Returns the complete list of measured items with offsets set.
    pub fn measure_all(
        &mut self,
        first_item_index: usize,
        first_item_scroll_offset: f32,
    ) -> Vec<LazyListMeasuredItem> {
        let start_time = std::time::Instant::now();
        let start_offset = self.config.before_content_padding - first_item_scroll_offset;
        let viewport_end = self.effective_viewport_size - self.config.after_content_padding;

        // Measure visible items
        let (mut visible_items, current_index, current_offset) =
            self.measure_visible(first_item_index, start_offset, viewport_end, start_time);

        // Measure beyond-bounds items after visible
        self.measure_beyond_after(
            current_index,
            current_offset,
            &mut visible_items,
            start_time,
        );

        // Measure beyond-bounds items before visible
        if first_item_index > 0 && !visible_items.is_empty() {
            let before_items =
                self.measure_beyond_before(first_item_index, visible_items[0].offset, start_time);
            if !before_items.is_empty() {
                let mut combined = before_items;
                combined.append(&mut visible_items);
                visible_items = combined;
            }
        }

        visible_items
    }

    /// Measures visible items starting from `start_index`.
    ///
    /// Returns (items, next_index, next_offset).
    fn measure_visible(
        &mut self,
        start_index: usize,
        start_offset: f32,
        viewport_end: f32,
        start_time: std::time::Instant,
    ) -> (Vec<LazyListMeasuredItem>, usize, f32) {
        let mut items = Vec::new();
        let mut current_index = start_index;
        let mut current_offset = start_offset;

        while current_index < self.items_count
            && current_offset < viewport_end
            && items.len() < MAX_VISIBLE_ITEMS_SAFETY
        {
            // Check time budget
            if start_time.elapsed() > DEFAULT_TIME_BUDGET {
                log::warn!(
                    "Lazy list measurement exceeded time budget ({:?}) at index {}. stopping early.",
                    DEFAULT_TIME_BUDGET,
                    current_index
                );
                break;
            }

            let mut item = self
                .take_pre_measured(current_index)
                .unwrap_or_else(|| (self.measure_fn)(current_index));
            item.offset = current_offset;
            current_offset += item.main_axis_size + self.config.spacing;
            items.push(item);
            current_index += 1;
        }

        // Warn if we hit the safety limit - viewport may be under-filled
        if items.len() >= MAX_VISIBLE_ITEMS_SAFETY && current_offset < viewport_end {
            log::warn!(
                "MAX_VISIBLE_ITEMS ({}) reached while viewport has remaining space ({:.0}px) - \
                 viewport may be under-filled. Consider using larger items.",
                MAX_VISIBLE_ITEMS_SAFETY,
                viewport_end - current_offset
            );
        }

        (items, current_index, current_offset)
    }

    /// Measures beyond-bounds items after visible items.
    fn measure_beyond_after(
        &mut self,
        mut current_index: usize,
        mut current_offset: f32,
        items: &mut Vec<LazyListMeasuredItem>,
        start_time: std::time::Instant,
    ) {
        let after_count = self
            .config
            .beyond_bounds_item_count
            .min(self.items_count - current_index);

        for _ in 0..after_count {
            if current_index >= self.items_count {
                break;
            }
            // Check time budget even for beyond-bounds
            if start_time.elapsed() > DEFAULT_TIME_BUDGET {
                break;
            }
            let mut item = self
                .take_pre_measured(current_index)
                .unwrap_or_else(|| (self.measure_fn)(current_index));
            item.offset = current_offset;
            current_offset += item.main_axis_size + self.config.spacing;
            items.push(item);
            current_index += 1;
        }
    }

    /// Measures beyond-bounds items before visible items.
    ///
    /// Returns items in correct order (earliest index first).
    fn measure_beyond_before(
        &mut self,
        first_index: usize,
        first_offset: f32,
        start_time: std::time::Instant,
    ) -> Vec<LazyListMeasuredItem> {
        let before_count = self.config.beyond_bounds_item_count.min(first_index);
        let mut before_items = Vec::with_capacity(before_count);
        let mut before_offset = first_offset;

        for i in 0..before_count {
            // Check time budget
            if start_time.elapsed() > DEFAULT_TIME_BUDGET {
                break;
            }
            let idx = first_index - 1 - i;
            let mut item = self
                .take_pre_measured(idx)
                .unwrap_or_else(|| (self.measure_fn)(idx));
            before_offset -= item.main_axis_size + self.config.spacing;
            item.offset = before_offset;
            before_items.push(item);
        }

        before_items.reverse();
        before_items
    }

    fn take_pre_measured(&mut self, index: usize) -> Option<LazyListMeasuredItem> {
        let front_matches = self
            .pre_measured
            .front()
            .is_some_and(|item| item.index == index);
        if front_matches {
            self.pre_measured.pop_front()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn create_test_item(index: usize, size: f32) -> LazyListMeasuredItem {
        LazyListMeasuredItem::new(index, index as u64, None, size, 100.0)
    }

    #[test]
    fn test_measure_fills_viewport() {
        let config = LazyListMeasureConfig::default();
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, VecDeque::new());

        let items = measurer.measure_all(0, 0.0);

        // 200px viewport / 50px items = 4 visible + 2 beyond = 6
        assert!(items.len() >= 4);
        assert_eq!(items[0].index, 0);
        assert_eq!(items[0].offset, 0.0);
    }

    #[test]
    fn test_measure_with_offset() {
        let config = LazyListMeasureConfig::default();
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, VecDeque::new());

        let items = measurer.measure_all(5, 25.0);

        // First visible item should be at index 5
        // Beyond-bounds before should include items 3, 4
        assert!(items.iter().any(|i| i.index == 3));
        assert!(items.iter().any(|i| i.index == 5));
    }

    #[test]
    fn test_measure_respects_items_count() {
        let config = LazyListMeasureConfig::default();
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer = ItemMeasurer::new(&mut measure, &config, 3, 1000.0, VecDeque::new());

        let items = measurer.measure_all(0, 0.0);

        // Only 3 items exist, even though viewport can fit more
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_measure_with_spacing() {
        let config = LazyListMeasureConfig {
            spacing: 10.0,
            ..Default::default()
        };
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, VecDeque::new());

        let items = measurer.measure_all(0, 0.0);

        // Check spacing is applied
        assert_eq!(items[0].offset, 0.0);
        assert_eq!(items[1].offset, 60.0); // 50 + 10 spacing
    }
}
