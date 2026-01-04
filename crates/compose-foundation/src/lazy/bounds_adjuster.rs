//! Scroll bounds adjustment for lazy list.
//!
//! This module handles clamping item offsets when scrolled past list bounds.

use super::lazy_list_measure::LazyListMeasureConfig;
use super::lazy_list_measured_item::LazyListMeasuredItem;

/// Adjusts item offsets to clamp within scroll bounds.
///
/// Prevents scrolling past the first/last item by adjusting offsets.
pub struct BoundsAdjuster<'a> {
    config: &'a LazyListMeasureConfig,
    items_count: usize,
    effective_viewport_size: f32,
}

impl<'a> BoundsAdjuster<'a> {
    /// Creates a new BoundsAdjuster.
    pub fn new(
        config: &'a LazyListMeasureConfig,
        items_count: usize,
        effective_viewport_size: f32,
    ) -> Self {
        Self {
            config,
            items_count,
            effective_viewport_size,
        }
    }

    /// Clamps items at both start and end bounds.
    pub fn clamp(&self, items: &mut [LazyListMeasuredItem]) {
        self.clamp_at_start(items);
        self.clamp_at_end(items);
    }

    /// Clamps items if scrolled past the first item.
    ///
    /// If the first item (index 0) is positioned past the content padding,
    /// adjusts all items to align with the start.
    pub fn clamp_at_start(&self, items: &mut [LazyListMeasuredItem]) {
        if items.is_empty() {
            return;
        }

        let first = &items[0];
        if first.index == 0 && first.offset > self.config.before_content_padding {
            let adjustment = first.offset - self.config.before_content_padding;
            for item in items.iter_mut() {
                item.offset -= adjustment;
            }
        }
    }

    /// Clamps items if scrolled past the last item.
    ///
    /// If the last item ends above the viewport bottom, adjusts all items
    /// to prevent scrolling past the end.
    pub fn clamp_at_end(&self, items: &mut [LazyListMeasuredItem]) {
        if items.is_empty() {
            return;
        }

        let last = items.last().unwrap();
        let last_item_end = last.offset + last.main_axis_size;
        let viewport_end = self.effective_viewport_size - self.config.after_content_padding;

        // Only clamp if this is the actual last item and it ends above viewport bottom
        if last.index == self.items_count - 1 && last_item_end < viewport_end {
            let adjustment = viewport_end - last_item_end;

            // Only adjust if we wouldn't push first item above start
            let first_offset_after = items[0].offset + adjustment;
            if first_offset_after <= self.config.before_content_padding || items[0].index > 0 {
                for item in items.iter_mut() {
                    item.offset += adjustment;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_item(index: usize, offset: f32, size: f32) -> LazyListMeasuredItem {
        let mut item = LazyListMeasuredItem::new(index, index as u64, None, size, 100.0);
        item.offset = offset;
        item
    }

    #[test]
    fn test_clamp_at_start() {
        let config = LazyListMeasureConfig::default();
        let adjuster = BoundsAdjuster::new(&config, 10, 500.0);

        // Item 0 positioned at 50.0 (past content padding of 0)
        let mut items = vec![
            create_test_item(0, 50.0, 100.0),
            create_test_item(1, 150.0, 100.0),
        ];

        adjuster.clamp_at_start(&mut items);

        assert_eq!(items[0].offset, 0.0);
        assert_eq!(items[1].offset, 100.0);
    }

    #[test]
    fn test_clamp_at_start_with_content_padding() {
        let config = LazyListMeasureConfig {
            before_content_padding: 20.0,
            ..Default::default()
        };
        let adjuster = BoundsAdjuster::new(&config, 10, 500.0);

        let mut items = vec![
            create_test_item(0, 70.0, 100.0),
            create_test_item(1, 170.0, 100.0),
        ];

        adjuster.clamp_at_start(&mut items);

        assert_eq!(items[0].offset, 20.0);
        assert_eq!(items[1].offset, 120.0);
    }

    #[test]
    fn test_clamp_at_end() {
        let config = LazyListMeasureConfig::default();
        let adjuster = BoundsAdjuster::new(&config, 5, 500.0);

        // Items starting at index 3 (not at list start), last item ends at 300
        // Since first item index > 0, clamping is allowed
        let mut items = vec![
            create_test_item(3, 100.0, 100.0),
            create_test_item(4, 200.0, 100.0), // Last item in list (index 4, items_count=5)
        ];

        adjuster.clamp_at_end(&mut items);

        // Items should be pushed down so last item ends at 500
        assert_eq!(items[1].offset + items[1].main_axis_size, 500.0);
    }

    #[test]
    fn test_no_clamp_when_not_at_bounds() {
        let config = LazyListMeasureConfig::default();
        let adjuster = BoundsAdjuster::new(&config, 100, 500.0);

        // Item 5 positioned correctly (not at start or end)
        let mut items = vec![
            create_test_item(5, 0.0, 100.0),
            create_test_item(6, 100.0, 100.0),
        ];

        adjuster.clamp(&mut items);

        // No adjustment should be made
        assert_eq!(items[0].offset, 0.0);
        assert_eq!(items[1].offset, 100.0);
    }
}
