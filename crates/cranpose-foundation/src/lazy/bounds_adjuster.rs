use super::{
    lazy_list_measure::LazyListMeasureConfig, lazy_list_measured_item::LazyListMeasuredItem,
};

pub struct BoundsAdjuster<'a> {
    config: &'a LazyListMeasureConfig,
    items_count: usize,
    effective_viewport_size: f32,
}

impl<'a> BoundsAdjuster<'a> {
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

    pub fn clamp(&self, items: &mut [LazyListMeasuredItem]) {
        self.clamp_at_start(items);
        self.clamp_at_end(items);
    }

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

    pub fn clamp_at_end(&self, items: &mut [LazyListMeasuredItem]) {
        if items.is_empty() {
            return;
        }

        let Some(last) = items.last() else {
            return;
        };
        let last_item_end = last.offset + last.main_axis_size;
        let viewport_end = self.effective_viewport_size - self.config.after_content_padding;

        if last.index == self.items_count - 1 && last_item_end < viewport_end {
            let adjustment = viewport_end - last_item_end;

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
#[path = "tests/bounds_adjuster_tests.rs"]
mod tests;
