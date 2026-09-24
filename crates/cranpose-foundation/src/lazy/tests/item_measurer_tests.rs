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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 120.0, 40.0, VecDeque::new());

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
fn fill_end_gap_backfills_fully_even_when_item_measurement_is_slow() {
    let config = LazyListMeasureConfig::default();
    let mut measure = |i| {
        std::thread::sleep(Duration::from_millis(2));
        create_test_item(i, 40.0)
    };
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 20, 500.0, 40.0, VecDeque::new())
        .with_include_before_beyond_bounds(false);

    let pass = measurer.measure_all(15, 0.0);

    assert_eq!(
        pass.start_index, 7,
        "the backfill must pull in every preceding item needed to fill the \
         grown viewport, not stop partway because measuring was slow: {pass:?}"
    );
    let indices: Vec<usize> = pass.items.iter().map(|item| item.index).collect();
    assert_eq!(indices, (7..=19).collect::<Vec<_>>());
    let last = pass.items.last().expect("measured items");
    let last_end = last.offset + last.main_axis_size;
    assert!(
        (last_end - 500.0).abs() < 0.01,
        "content bottom must align with the grown viewport end: {last_end}"
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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 120.0, 40.0, VecDeque::new())
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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 120.0, 40.0, VecDeque::new())
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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 80.0, 40.0, VecDeque::new())
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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new())
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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 3, 1000.0, 50.0, VecDeque::new());

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
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

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
    let measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, f32::NAN, VecDeque::new());

    assert_eq!(measurer.estimated_measure_capacity(0, 0.0, 96.0), 4);
}

#[test]
fn measure_beyond_before_reserves_capacity_for_following_items() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 2,
        ..Default::default()
    };
    let mut measure = |i| create_test_item(i, 50.0);
    let mut measurer = ItemMeasurer::new(&mut measure, &config, 100, 200.0, 50.0, VecDeque::new());

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
