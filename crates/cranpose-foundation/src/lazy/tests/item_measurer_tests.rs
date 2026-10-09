use std::collections::VecDeque;

use super::*;

fn create_test_item(index: usize, size: f32) -> LazyListMeasuredItem {
    LazyListMeasuredItem::new(index, index as u64, None, size, 100.0)
}

/// Items measured by `measure` and kept beyond the viewport by `keep`.
struct Source<M, K> {
    measure: M,
    keep: K,
}

impl<M, K> LazyItemSource for Source<M, K>
where
    M: FnMut(usize) -> LazyListMeasuredItem,
    K: FnMut(usize) -> bool,
{
    fn measure(&mut self, index: usize) -> LazyListMeasuredItem {
        (self.measure)(index)
    }

    fn keep_beyond(&mut self, index: usize) -> BeyondItem {
        if (self.keep)(index) {
            BeyondItem::Kept
        } else {
            BeyondItem::Declined
        }
    }
}

fn slow_first_item(
    kept: &mut Vec<usize>,
) -> Source<impl FnMut(usize) -> LazyListMeasuredItem, impl FnMut(usize) -> bool + '_> {
    Source {
        measure: |i| {
            if i == 0 {
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            create_test_item(i, 40.0)
        },
        keep: move |i| {
            kept.push(i);
            true
        },
    }
}

fn sized(
    size: f32,
) -> Source<impl FnMut(usize) -> LazyListMeasuredItem, impl FnMut(usize) -> bool> {
    Source {
        measure: move |i| create_test_item(i, size),
        keep: |_| true,
    }
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
    let mut kept = Vec::new();
    let mut source = slow_first_item(&mut kept);
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 120.0, 40.0, VecDeque::new());

    let pass = measurer.measure_all(0, 0.0);

    assert!(
        pass.viewport_filled,
        "visible lazy measurement must not leave a blank viewport when the budget is exceeded"
    );
    assert_eq!(pass.measured_visible_items, 3);
    assert_eq!(
        pass.items.iter().map(|item| item.index).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    drop(source);
    assert_eq!(
        kept,
        vec![3, 4],
        "the configured beyond-bounds items stay composed however long the pass took"
    );
}

#[test]
fn fill_end_gap_backfills_fully_even_when_item_measurement_is_slow() {
    let config = LazyListMeasureConfig::default();
    let mut source = Source {
        measure: |i| {
            std::thread::sleep(Duration::from_millis(2));
            create_test_item(i, 40.0)
        },
        keep: |_| true,
    };
    let mut measurer = ItemMeasurer::new(&mut source, &config, 20, 500.0, 40.0, VecDeque::new())
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
    let mut kept = Vec::new();
    let mut source = slow_first_item(&mut kept);
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 120.0, 40.0, VecDeque::new())
        .with_beyond_bounds_item_count(6);

    let pass = measurer.measure_all(0, 0.0);

    assert_eq!(
        pass.items.iter().map(|item| item.index).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    drop(source);
    assert_eq!(
        kept,
        vec![3, 4],
        "elapsed budget should stop only adaptive extra beyond-bounds work"
    );
}

#[test]
fn declined_beyond_items_never_block_visible_measurement() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 2,
        ..LazyListMeasureConfig::default()
    };
    let mut measured = Vec::new();
    let mut source = Source {
        measure: |i| {
            measured.push(i);
            create_test_item(i, 40.0)
        },
        keep: |_| false,
    };
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 120.0, 40.0, VecDeque::new());

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
fn beyond_items_are_kept_until_the_first_one_declined() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 4,
        ..LazyListMeasureConfig::default()
    };
    let mut asked = Vec::new();
    let mut source = Source {
        measure: |i| create_test_item(i, 40.0),
        keep: |i| {
            asked.push(i);
            i < 4
        },
    };
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 80.0, 40.0, VecDeque::new());

    let pass = measurer.measure_all(0, 0.0);

    assert_eq!(pass.measured_visible_items, 2);
    assert_eq!(
        pass.items.iter().map(|item| item.index).collect::<Vec<_>>(),
        vec![0, 1],
        "items beyond the viewport are not placed"
    );
    assert_eq!(asked, vec![2, 3, 4]);
}

#[test]
fn test_measure_fills_viewport() {
    let config = LazyListMeasureConfig::default();
    let mut source = sized(50.0);
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 200.0, 50.0, VecDeque::new());

    let pass = measurer.measure_all(0, 0.0);
    let items = pass.items;

    assert!(items.len() >= 4);
    assert_eq!(items[0].index, 0);
    assert_eq!(items[0].offset, 0.0);
}

#[test]
fn test_measure_with_offset() {
    let config = LazyListMeasureConfig::default();
    let mut kept = Vec::new();
    let mut source = Source {
        measure: |i| create_test_item(i, 50.0),
        keep: |i| {
            kept.push(i);
            true
        },
    };
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 200.0, 50.0, VecDeque::new());

    let pass = measurer.measure_all(5, 25.0);

    assert_eq!(
        pass.items.iter().map(|item| item.index).collect::<Vec<_>>(),
        vec![5, 6, 7, 8, 9]
    );
    assert_eq!(kept, vec![10, 11, 4, 3]);
}

#[test]
fn forward_scroll_measurement_can_skip_before_beyond_bounds() {
    let config = LazyListMeasureConfig::default();
    let mut reached = Vec::new();
    let mut kept = Vec::new();
    let mut source = Source {
        measure: |i| {
            reached.push(i);
            create_test_item(i, 50.0)
        },
        keep: |i| {
            kept.push(i);
            true
        },
    };
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 200.0, 50.0, VecDeque::new())
        .with_include_before_beyond_bounds(false);

    measurer.measure_all(5, 25.0);
    reached.extend(kept);

    assert!(
        reached.iter().all(|index| *index >= 5),
        "offscreen before-buffer rows must not be reached during forward scroll: {reached:?}"
    );
}

#[test]
fn test_measure_respects_items_count() {
    let config = LazyListMeasureConfig::default();
    let mut source = sized(50.0);
    let mut measurer = ItemMeasurer::new(&mut source, &config, 3, 1000.0, 50.0, VecDeque::new());

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
    let mut source = sized(50.0);
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 200.0, 50.0, VecDeque::new());

    let pass = measurer.measure_all(0, 0.0);
    let items = pass.items;

    assert_eq!(items[0].offset, 0.0);
    assert_eq!(items[1].offset, 60.0);
}

#[test]
fn placed_beyond_items_sit_on_both_sides_of_the_viewport_items() {
    struct PlaceAll;
    impl LazyItemSource for PlaceAll {
        fn measure(&mut self, index: usize) -> LazyListMeasuredItem {
            create_test_item(index, 50.0)
        }

        fn keep_beyond(&mut self, index: usize) -> BeyondItem {
            BeyondItem::Placed(create_test_item(index, 50.0))
        }
    }
    let config = LazyListMeasureConfig {
        spacing: 10.0,
        ..LazyListMeasureConfig::default()
    };
    let mut source = PlaceAll;
    let mut measurer = ItemMeasurer::new(&mut source, &config, 100, 100.0, 50.0, VecDeque::new());

    let pass = measurer.measure_all(5, 0.0);

    assert_eq!(
        pass.items
            .iter()
            .map(|item| (item.index, item.offset))
            .collect::<Vec<_>>(),
        vec![
            (3, -120.0),
            (4, -60.0),
            (5, 0.0),
            (6, 60.0),
            (7, 120.0),
            (8, 180.0)
        ]
    );
}
