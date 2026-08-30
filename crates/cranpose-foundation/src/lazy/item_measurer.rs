use std::collections::VecDeque;

use web_time::{Duration, Instant};

use super::{
    lazy_list_measure::{DEFAULT_ITEM_SIZE_ESTIMATE, LazyListMeasureConfig},
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

pub trait BeyondBoundsMeasurePolicy {
    fn should_measure_beyond_item(&mut self, index: usize) -> bool;
}

pub struct AlwaysMeasureBeyond;

impl BeyondBoundsMeasurePolicy for AlwaysMeasureBeyond {
    fn should_measure_beyond_item(&mut self, _index: usize) -> bool {
        true
    }
}

impl<F> BeyondBoundsMeasurePolicy for F
where
    F: FnMut(usize) -> bool,
{
    fn should_measure_beyond_item(&mut self, index: usize) -> bool {
        self(index)
    }
}

pub struct ItemMeasurer<'a, F, B = AlwaysMeasureBeyond> {
    measure_fn: &'a mut F,
    beyond_bounds_policy: B,
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

impl<'a, F> ItemMeasurer<'a, F, AlwaysMeasureBeyond>
where
    F: FnMut(usize) -> LazyListMeasuredItem,
{
    pub fn new(
        measure_fn: &'a mut F,
        config: &'a LazyListMeasureConfig,
        items_count: usize,
        effective_viewport_size: f32,
        average_item_size: f32,
        pre_measured: VecDeque<LazyListMeasuredItem>,
    ) -> Self {
        Self {
            measure_fn,
            beyond_bounds_policy: AlwaysMeasureBeyond,
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
}

impl<'a, F, B> ItemMeasurer<'a, F, B>
where
    F: FnMut(usize) -> LazyListMeasuredItem,
    B: BeyondBoundsMeasurePolicy,
{
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

    pub fn with_beyond_bounds_measure_policy<N>(self, policy: N) -> ItemMeasurer<'a, F, N>
    where
        N: BeyondBoundsMeasurePolicy,
    {
        ItemMeasurer {
            measure_fn: self.measure_fn,
            beyond_bounds_policy: policy,
            pre_measured: self.pre_measured,
            config: self.config,
            guaranteed_after_beyond_bounds_item_count: self
                .guaranteed_after_beyond_bounds_item_count,
            guaranteed_before_beyond_bounds_item_count: self
                .guaranteed_before_beyond_bounds_item_count,
            beyond_bounds_item_count: self.beyond_bounds_item_count,
            items_count: self.items_count,
            effective_viewport_size: self.effective_viewport_size,
            average_item_size: self.average_item_size,
            include_before_beyond_bounds: self.include_before_beyond_bounds,
            telemetry_pass_id: self.telemetry_pass_id,
        }
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
                start_time,
            )
            .unwrap_or(first_item_index);
        let backfilled = effective_first_index != first_item_index;

        self.measure_beyond_after(
            current_index,
            current_offset,
            &mut visible_items,
            start_time,
        );

        if self.include_before_beyond_bounds
            && effective_first_index > 0
            && !visible_items.is_empty()
        {
            let before_items = self.measure_beyond_before(
                effective_first_index,
                visible_items[0].offset,
                visible_items.len(),
                start_time,
            );
            if !before_items.is_empty() {
                let mut combined = before_items;
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
        start_time: Instant,
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
        while idx > 0 && top > self.config.before_content_padding + 0.5 {
            if start_time.elapsed() > DEFAULT_TIME_BUDGET {
                break;
            }
            idx -= 1;
            let mut item = self
                .take_pre_measured(idx)
                .unwrap_or_else(|| (self.measure_fn)(idx));
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
                .unwrap_or_else(|| (self.measure_fn)(current_index));
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

    fn measure_beyond_after(
        &mut self,
        mut current_index: usize,
        mut current_offset: f32,
        items: &mut Vec<LazyListMeasuredItem>,
        start_time: Instant,
    ) {
        let after_count = self
            .beyond_bounds_item_count
            .min(self.items_count - current_index);

        for measured_after in 0..after_count {
            if current_index >= self.items_count {
                break;
            }
            if measured_after >= self.guaranteed_after_beyond_bounds_item_count
                && start_time.elapsed() > DEFAULT_TIME_BUDGET
            {
                break;
            }
            if !self
                .beyond_bounds_policy
                .should_measure_beyond_item(current_index)
            {
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

    fn measure_beyond_before(
        &mut self,
        first_index: usize,
        first_offset: f32,
        following_item_count: usize,
        start_time: Instant,
    ) -> Vec<LazyListMeasuredItem> {
        let before_count = self.beyond_bounds_item_count.min(first_index);
        if before_count == 0 {
            return Vec::new();
        }

        let mut before_items =
            Vec::with_capacity(before_count.saturating_add(following_item_count));
        let mut before_offset = first_offset;

        for i in 0..before_count {
            if i >= self.guaranteed_before_beyond_bounds_item_count
                && start_time.elapsed() > DEFAULT_TIME_BUDGET
            {
                break;
            }
            let idx = first_index - 1 - i;
            if !self.beyond_bounds_policy.should_measure_beyond_item(idx) {
                break;
            }
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

        estimated_visible
            .saturating_add(self.beyond_bounds_item_count)
            .min(remaining_items)
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
mod tests {
    use std::collections::VecDeque;

    use super::*;

    fn create_test_item(index: usize, size: f32) -> LazyListMeasuredItem {
        LazyListMeasuredItem::new(index, index as u64, None, size, 100.0)
    }

    #[test]
    fn measure_time_budget_fits_120hz_frame_budget() {
        assert!(
            DEFAULT_TIME_BUDGET <= Duration::from_millis(6),
            "lazy measurement cannot reserve an entire 120Hz frame"
        );
    }

    #[test]
    fn visible_measurement_fills_viewport_even_when_item_work_exceeds_budget() {
        let config = LazyListMeasureConfig::default();
        let mut measured = 0usize;
        let mut measure = |i| {
            measured += 1;
            if i == 0 {
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            create_test_item(i, 40.0)
        };
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 120.0, 40.0, VecDeque::new());

        let pass = measurer.measure_all(0, 0.0);

        assert!(
            pass.viewport_filled,
            "visible lazy measurement must not leave a blank viewport when the budget is exceeded"
        );
        assert_eq!(pass.measured_visible_items, 3);
        assert_eq!(
            pass.items.iter().map(|item| item.index).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(
            measured, 5,
            "configured beyond-bounds rows are part of deterministic retained layout"
        );
    }

    #[test]
    fn adaptive_extra_beyond_bounds_is_budgeted_after_configured_window() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 2,
            ..LazyListMeasureConfig::default()
        };
        let mut measured = 0usize;
        let mut measure = |i| {
            measured += 1;
            if i == 0 {
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            create_test_item(i, 40.0)
        };
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 120.0, 40.0, VecDeque::new())
                .with_beyond_bounds_item_count(6);

        let pass = measurer.measure_all(0, 0.0);

        assert_eq!(
            pass.items.iter().map(|item| item.index).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(
            measured, 5,
            "elapsed budget should stop only adaptive extra beyond-bounds work"
        );
    }

    #[test]
    fn beyond_policy_never_blocks_visible_measurement() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 2,
            ..LazyListMeasureConfig::default()
        };
        let mut measured = Vec::new();
        let mut measure = |i| {
            measured.push(i);
            create_test_item(i, 40.0)
        };
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 120.0, 40.0, VecDeque::new())
                .with_beyond_bounds_measure_policy(|_index| false);

        let pass = measurer.measure_all(0, 0.0);

        assert!(pass.viewport_filled);
        assert_eq!(pass.measured_visible_items, 3);
        assert_eq!(
            pass.items.iter().map(|item| item.index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(measured, vec![0, 1, 2]);
    }

    #[test]
    fn beyond_policy_uses_ready_frontier_until_first_miss() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 4,
            ..LazyListMeasureConfig::default()
        };
        let mut measured = Vec::new();
        let mut measure = |i| {
            measured.push(i);
            create_test_item(i, 40.0)
        };
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 80.0, 40.0, VecDeque::new())
                .with_beyond_bounds_measure_policy(|index| index < 5);

        let pass = measurer.measure_all(0, 0.0);

        assert_eq!(pass.measured_visible_items, 2);
        assert_eq!(
            pass.items.iter().map(|item| item.index).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
        assert_eq!(measured, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_measure_fills_viewport() {
        let config = LazyListMeasureConfig::default();
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

        let pass = measurer.measure_all(0, 0.0);
        let items = pass.items;

        assert!(items.len() >= 4);
        assert_eq!(items[0].index, 0);
        assert_eq!(items[0].offset, 0.0);
    }

    #[test]
    fn test_measure_with_offset() {
        let config = LazyListMeasureConfig::default();
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

        let pass = measurer.measure_all(5, 25.0);
        let items = pass.items;

        assert!(items.iter().any(|i| i.index == 3));
        assert!(items.iter().any(|i| i.index == 5));
    }

    #[test]
    fn forward_scroll_measurement_can_skip_before_beyond_bounds() {
        let config = LazyListMeasureConfig::default();
        let mut measured = Vec::new();
        let mut measure = |i| {
            measured.push(i);
            create_test_item(i, 50.0)
        };
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new())
                .with_include_before_beyond_bounds(false);

        let pass = measurer.measure_all(5, 25.0);
        let indices = pass.items.iter().map(|item| item.index).collect::<Vec<_>>();

        assert!(
            indices.iter().all(|index| *index >= 5),
            "forward scroll should not spend frame budget measuring offscreen before-buffer rows: {indices:?}"
        );
        assert!(
            measured.iter().all(|index| *index >= 5),
            "offscreen before-buffer rows must not be measured during forward scroll: {measured:?}"
        );
    }

    #[test]
    fn test_measure_respects_items_count() {
        let config = LazyListMeasureConfig::default();
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 3, 1000.0, 50.0, VecDeque::new());

        let pass = measurer.measure_all(0, 0.0);
        let items = pass.items;

        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_measure_with_spacing() {
        let config = LazyListMeasureConfig {
            spacing: 10.0,
            ..Default::default()
        };
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

        let pass = measurer.measure_all(0, 0.0);
        let items = pass.items;

        assert_eq!(items[0].offset, 0.0);
        assert_eq!(items[1].offset, 60.0);
    }

    #[test]
    fn measure_capacity_uses_average_item_size_and_beyond_bounds() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 2,
            ..Default::default()
        };
        let mut measure = |i| create_test_item(i, 50.0);
        let measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

        assert_eq!(measurer.estimated_measure_capacity(0, 0.0, 200.0), 6);
    }

    #[test]
    fn measure_capacity_falls_back_for_invalid_average_item_size() {
        let config = LazyListMeasureConfig::default();
        let mut measure = |i| create_test_item(i, 50.0);
        let measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 200.0, f32::NAN, VecDeque::new());

        assert_eq!(measurer.estimated_measure_capacity(0, 0.0, 96.0), 4);
    }

    #[test]
    fn measure_beyond_before_reserves_capacity_for_following_items() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 2,
            ..Default::default()
        };
        let mut measure = |i| create_test_item(i, 50.0);
        let mut measurer =
            ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

        let before_items = measurer.measure_beyond_before(5, 0.0, 6, Instant::now());

        assert_eq!(
            before_items
                .iter()
                .map(|item| item.index)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert!(before_items.capacity() >= 8);
    }
}
