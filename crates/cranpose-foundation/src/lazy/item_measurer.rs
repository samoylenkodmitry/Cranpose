use std::collections::VecDeque;

use web_time::{Duration, Instant};

use super::{
    lazy_list_measure::{
        BeyondItem, DEFAULT_ITEM_SIZE_ESTIMATE, LazyItemSource, LazyListMeasureConfig,
    },
    lazy_list_measured_item::LazyListMeasuredItem,
};

const DEFAULT_TIME_BUDGET: Duration = Duration::from_millis(6);

const MAX_VISIBLE_ITEMS_SAFETY: usize = 10000;
const MIN_ESTIMATED_ITEM_EXTENT: f32 = 1.0;

#[derive(Debug)]
pub struct ItemMeasurePass {
    pub items: Vec<LazyListMeasuredItem>,
    pub start_index: usize,
    pub start_offset: f32,
    pub next_index: usize,
    pub next_offset: f32,
    pub measured_visible_items: usize,
    pub hit_time_budget: bool,
    pub viewport_filled: bool,
}

pub struct ItemMeasurer<'a, S> {
    source: &'a mut S,
    pre_measured: VecDeque<LazyListMeasuredItem>,
    config: &'a LazyListMeasureConfig,
    guaranteed_after_beyond_bounds_item_count: usize,
    guaranteed_before_beyond_bounds_item_count: usize,
    beyond_bounds_item_count: usize,
    items_count: usize,
    effective_viewport_size: f32,
    average_item_size: f32,
    include_before_beyond_bounds: bool,
    telemetry_pass_id: Option<u64>,
}

impl<'a, S> ItemMeasurer<'a, S>
where
    S: LazyItemSource,
{
    pub fn new(
        source: &'a mut S,
        config: &'a LazyListMeasureConfig,
        items_count: usize,
        effective_viewport_size: f32,
        average_item_size: f32,
        pre_measured: VecDeque<LazyListMeasuredItem>,
    ) -> Self {
        Self {
            source,
            pre_measured,
            config,
            guaranteed_after_beyond_bounds_item_count: config.beyond_bounds_item_count,
            guaranteed_before_beyond_bounds_item_count: config.beyond_bounds_item_count,
            beyond_bounds_item_count: config.beyond_bounds_item_count,
            items_count,
            effective_viewport_size,
            average_item_size,
            include_before_beyond_bounds: true,
            telemetry_pass_id: None,
        }
    }

    pub fn with_beyond_bounds_item_count(mut self, count: usize) -> Self {
        self.beyond_bounds_item_count = count;
        self
    }

    pub fn with_guaranteed_after_beyond_bounds_item_count(mut self, count: usize) -> Self {
        self.guaranteed_after_beyond_bounds_item_count = count.min(self.beyond_bounds_item_count);
        self
    }

    pub fn with_include_before_beyond_bounds(mut self, include: bool) -> Self {
        self.include_before_beyond_bounds = include;
        self
    }

    pub fn with_telemetry_pass_id(mut self, pass_id: Option<u64>) -> Self {
        self.telemetry_pass_id = pass_id;
        self
    }

    pub fn measure_all(
        &mut self,
        first_item_index: usize,
        first_item_scroll_offset: f32,
    ) -> ItemMeasurePass {
        let start_time = Instant::now();
        let leading_padding = if first_item_index == 0 {
            self.config.before_content_padding
        } else {
            0.0
        };
        let start_offset = leading_padding - first_item_scroll_offset;
        let viewport_end = self.effective_viewport_size;
        let content_end =
            (self.effective_viewport_size - self.config.after_content_padding).max(0.0);

        let (mut visible_items, current_index, current_offset) =
            self.measure_visible(first_item_index, start_offset, viewport_end, start_time);
        let measured_visible_items = visible_items.len();
        let hit_time_budget = start_time.elapsed() > DEFAULT_TIME_BUDGET;

        let effective_first_index = self
            .fill_end_gap(
                &mut visible_items,
                current_index,
                current_offset,
                content_end,
                first_item_index,
            )
            .unwrap_or(first_item_index);
        let backfilled = effective_first_index != first_item_index;

        self.keep_beyond_after(
            current_index,
            current_offset,
            &mut visible_items,
            start_time,
        );

        if self.include_before_beyond_bounds
            && effective_first_index > 0
            && !visible_items.is_empty()
        {
            let placed_before =
                self.keep_beyond_before(effective_first_index, visible_items[0].offset, start_time);
            if !placed_before.is_empty() {
                let mut combined = placed_before;
                combined.append(&mut visible_items);
                visible_items = combined;
            }
        }

        let viewport_filled =
            backfilled || current_offset >= viewport_end || current_index >= self.items_count;
        let pass = ItemMeasurePass {
            items: visible_items,
            start_index: effective_first_index,
            start_offset,
            next_index: current_index,
            next_offset: current_offset,
            measured_visible_items,
            hit_time_budget,
            viewport_filled,
        };
        if let Some(pass_id) = self.telemetry_pass_id {
            log::warn!(
                "[lazy-measure-telemetry] pass={} start_index={} start_offset={:.2} measured_visible={} next_index={} next_offset={:.2} viewport_end={:.2} viewport_filled={} hit_time_budget={} elapsed_ms={:.2}",
                pass_id,
                pass.start_index,
                pass.start_offset,
                pass.measured_visible_items,
                pass.next_index,
                pass.next_offset,
                viewport_end,
                pass.viewport_filled,
                pass.hit_time_budget,
                start_time.elapsed().as_secs_f64() * 1000.0
            );
        }
        pass
    }

    fn fill_end_gap(
        &mut self,
        visible_items: &mut Vec<LazyListMeasuredItem>,
        current_index: usize,
        current_offset: f32,
        viewport_end: f32,
        first_item_index: usize,
    ) -> Option<usize> {
        if current_index < self.items_count || first_item_index == 0 || visible_items.is_empty() {
            return None;
        }
        let spare = viewport_end - current_offset;
        if spare <= 0.5 {
            return None;
        }
        for item in visible_items.iter_mut() {
            item.offset += spare;
        }
        let mut top = visible_items[0].offset;
        let mut idx = first_item_index;
        while idx > 0
            && top > self.config.before_content_padding + 0.5
            && visible_items.len() < MAX_VISIBLE_ITEMS_SAFETY
        {
            idx -= 1;
            let mut item = self
                .take_pre_measured(idx)
                .unwrap_or_else(|| self.source.measure(idx));
            top -= item.main_axis_size + self.config.spacing;
            item.offset = top;
            visible_items.insert(0, item);
        }
        let first = &visible_items[0];
        if first.index == 0 && first.offset > self.config.before_content_padding {
            let adjustment = first.offset - self.config.before_content_padding;
            for item in visible_items.iter_mut() {
                item.offset -= adjustment;
            }
        }
        Some(visible_items[0].index)
    }

    fn measure_visible(
        &mut self,
        start_index: usize,
        start_offset: f32,
        viewport_end: f32,
        _start_time: Instant,
    ) -> (Vec<LazyListMeasuredItem>, usize, f32) {
        let mut items = Vec::with_capacity(self.estimated_measure_capacity(
            start_index,
            start_offset,
            viewport_end,
        ));
        let mut current_index = start_index;
        let mut current_offset = start_offset;

        while current_index < self.items_count
            && current_offset < viewport_end
            && items.len() < MAX_VISIBLE_ITEMS_SAFETY
        {
            let mut item = self
                .take_pre_measured(current_index)
                .unwrap_or_else(|| self.source.measure(current_index));
            item.offset = current_offset;
            current_offset += item.main_axis_size + self.config.spacing;
            items.push(item);
            current_index += 1;
        }

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

    fn keep_beyond_after(
        &mut self,
        mut current_index: usize,
        mut current_offset: f32,
        items: &mut Vec<LazyListMeasuredItem>,
        start_time: Instant,
    ) {
        let after_count = self
            .beyond_bounds_item_count
            .min(self.items_count - current_index);

        for kept_after in 0..after_count {
            if kept_after >= self.guaranteed_after_beyond_bounds_item_count
                && start_time.elapsed() > DEFAULT_TIME_BUDGET
            {
                break;
            }
            match self.source.keep_beyond(current_index) {
                BeyondItem::Declined => break,
                BeyondItem::Kept => {}
                BeyondItem::Placed(mut item) => {
                    item.offset = current_offset;
                    current_offset += item.main_axis_size + self.config.spacing;
                    items.push(item);
                }
            }
            current_index += 1;
        }
    }

    fn keep_beyond_before(
        &mut self,
        first_index: usize,
        first_offset: f32,
        start_time: Instant,
    ) -> Vec<LazyListMeasuredItem> {
        let before_count = self.beyond_bounds_item_count.min(first_index);
        let mut placed = Vec::new();
        let mut before_offset = first_offset;
        for kept_before in 0..before_count {
            if kept_before >= self.guaranteed_before_beyond_bounds_item_count
                && start_time.elapsed() > DEFAULT_TIME_BUDGET
            {
                break;
            }
            match self.source.keep_beyond(first_index - 1 - kept_before) {
                BeyondItem::Declined => break,
                BeyondItem::Kept => {}
                BeyondItem::Placed(mut item) => {
                    before_offset -= item.main_axis_size + self.config.spacing;
                    item.offset = before_offset;
                    placed.push(item);
                }
            }
        }
        placed.reverse();
        placed
    }

    fn estimated_measure_capacity(
        &self,
        start_index: usize,
        start_offset: f32,
        viewport_end: f32,
    ) -> usize {
        let remaining_items = self
            .items_count
            .saturating_sub(start_index)
            .min(MAX_VISIBLE_ITEMS_SAFETY);
        if remaining_items == 0 || start_offset >= viewport_end {
            return 0;
        }

        let viewport_span = viewport_end - start_offset;
        if !viewport_span.is_finite() || viewport_span <= 0.0 {
            return 0;
        }

        let estimated_visible = (viewport_span / self.estimated_item_extent())
            .ceil()
            .min(MAX_VISIBLE_ITEMS_SAFETY as f32) as usize;

        estimated_visible.min(remaining_items)
    }

    fn estimated_item_extent(&self) -> f32 {
        let item_size = if self.average_item_size.is_finite() && self.average_item_size > 0.0 {
            self.average_item_size
        } else {
            DEFAULT_ITEM_SIZE_ESTIMATE
        };
        (item_size + self.config.spacing.max(0.0)).max(MIN_ESTIMATED_ITEM_EXTENT)
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
#[path = "tests/item_measurer_tests.rs"]
mod tests;
