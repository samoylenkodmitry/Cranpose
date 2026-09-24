use super::{
    super::lazy_list_state::test_helpers::{
        new_lazy_list_state, new_lazy_list_state_with_position, with_test_runtime,
    },
    *,
};

fn create_test_item(index: usize, size: f32) -> LazyListMeasuredItem {
    LazyListMeasuredItem::new(index, index as u64, None, size, 100.0)
}

#[test]
fn lazy_list_measure_config_defaults_to_two_beyond_bounds_items() {
    let config = LazyListMeasureConfig::default();

    assert_eq!(config.beyond_bounds_item_count, 2);
}

#[test]
fn active_scroll_guarantees_forward_warm_rows_for_single_wheel_ticks() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 0,
        spacing: 4.0,
        ..Default::default()
    };

    assert_eq!(
        adaptive_scroll_beyond_bounds_item_count(&config, -40.0, 48.0),
        2,
        "single wheel ticks should not force the full fast-scroll warm window"
    );
}

#[test]
fn default_single_wheel_scroll_uses_configured_markdown_warm_window() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 2,
        spacing: 8.0,
        ..Default::default()
    };

    assert_eq!(
        adaptive_scroll_beyond_bounds_item_count(&config, -40.0, 120.0),
        2,
        "small Markdown wheel ticks must not measure eight cached text rows every frame"
    );
}

#[test]
fn idle_measurement_warms_a_small_forward_window() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 0,
        spacing: 4.0,
        ..Default::default()
    };

    let adaptive = adaptive_scroll_beyond_bounds_item_count(&config, 0.0, 48.0);

    assert_eq!(adaptive, MIN_IDLE_WARM_BEYOND_BOUNDS_ITEMS);
}

#[test]
fn active_scroll_guarantees_forward_warm_rows_when_configured_buffer_is_zero() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 0,
        spacing: 4.0,
        ..Default::default()
    };

    let adaptive = adaptive_scroll_beyond_bounds_item_count(&config, -620.0, 48.0);

    assert_eq!(adaptive, MAX_ADAPTIVE_SCROLL_BEYOND_BOUNDS_ITEMS);
}

#[test]
fn adaptive_scroll_beyond_bounds_warms_fast_wheel_scroll_window() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 0,
        spacing: 4.0,
        ..Default::default()
    };

    assert_eq!(
        adaptive_scroll_beyond_bounds_item_count(&config, -620.0, 48.0),
        MAX_ADAPTIVE_SCROLL_BEYOND_BOUNDS_ITEMS
    );
}

#[test]
fn adaptive_scroll_beyond_bounds_never_shrinks_configured_buffer() {
    let config = LazyListMeasureConfig {
        beyond_bounds_item_count: 12,
        spacing: 4.0,
        ..Default::default()
    };

    assert_eq!(
        adaptive_scroll_beyond_bounds_item_count(&config, -620.0, 48.0),
        12
    );
}

fn exact_scroll_position(
    item_sizes: &[f32],
    spacing: f32,
    viewport_size: f32,
    deltas: &[f32],
) -> Vec<(usize, f32)> {
    let total_content = item_sizes
        .iter()
        .enumerate()
        .map(|(index, size)| {
            let spacing_after = if index + 1 < item_sizes.len() {
                spacing
            } else {
                0.0
            };
            size + spacing_after
        })
        .sum::<f32>();
    let max_scroll = (total_content - viewport_size).max(0.0);
    let mut scroll = 0.0f32;
    let mut positions = Vec::with_capacity(deltas.len());

    for delta in deltas {
        scroll = (scroll - delta).clamp(0.0, max_scroll);

        let mut remaining = scroll;
        let mut index = 0usize;
        while index + 1 < item_sizes.len() {
            let spacing_after = if index + 1 < item_sizes.len() {
                spacing
            } else {
                0.0
            };
            let extent = item_sizes[index] + spacing_after;
            if remaining < extent {
                break;
            }
            remaining -= extent;
            index += 1;
        }
        positions.push((index, remaining));
    }

    positions
}

#[test]
fn test_measure_empty_list() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(0, &state, 500.0, 300.0, &config, |_| {
            panic!("Should not measure any items");
        });

        assert!(result.visible_items.is_empty());
    });
}

#[test]
fn test_measure_single_item() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(1, &state, 500.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        assert_eq!(result.visible_items.len(), 1);
        assert_eq!(result.visible_items[0].index, 0);
        assert!(!result.can_scroll_forward);
        assert!(!result.can_scroll_backward);
    });
}

#[test]
fn test_measure_fills_viewport() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(10, &state, 200.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        assert!(result.visible_items.len() >= 4);
        assert!(result.can_scroll_forward);
        assert!(!result.can_scroll_backward);
    });
}

#[test]
fn test_measure_with_scroll_offset() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(3, 25.0);
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(20, &state, 200.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        assert_eq!(result.first_visible_item_index, 3);
        assert!(result.can_scroll_forward);
        assert!(result.can_scroll_backward);
    });
}

#[test]
fn test_backward_scroll_uses_measured_size() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(1, 0.0);
        state.dispatch_scroll_delta(1.0);
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(2, &state, 100.0, 300.0, &config, |i| {
            if i == 0 {
                create_test_item(i, 10.0)
            } else {
                create_test_item(i, 100.0)
            }
        });

        assert_eq!(result.first_visible_item_index, 0);
        assert!((result.first_visible_item_scroll_offset - 9.0).abs() < 0.001);
    });
}

#[test]
fn test_backward_scroll_with_spacing_preserves_offset_gap() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(1, 0.0);
        let config = LazyListMeasureConfig {
            spacing: 4.0,
            ..Default::default()
        };
        state.dispatch_scroll_delta(2.0);

        let result = measure_lazy_list(2, &state, 40.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        assert_eq!(result.first_visible_item_index, 0);
        assert!((result.first_visible_item_scroll_offset - 52.0).abs() < 0.001);
    });
}

#[test]
fn test_scroll_to_item() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        state.scroll_to_item(5, 0.0);

        let config = LazyListMeasureConfig::default();
        let result = measure_lazy_list(20, &state, 200.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        assert_eq!(result.first_visible_item_index, 5);
    });
}

#[test]
fn test_time_budget_fills_visible_viewport_and_keeps_configured_beyond_bounds() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(100, 5_000.0);
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(10_000, &state, 100.0, 300.0, &config, |i| {
            std::thread::sleep(std::time::Duration::from_millis(8));
            create_test_item(i, 10.0)
        });

        assert_eq!(
            result.first_visible_item_index, 212,
            "time-budgeted pass should report the first item that actually reaches the viewport"
        );
        assert!(
            (result.first_visible_item_scroll_offset - 4.0).abs() < 1.0,
            "expected actual visible offset to be preserved"
        );
        assert_eq!(
            result.visible_items.first().map(|item| item.index),
            Some(200),
            "measurement should keep the configured leading retained items"
        );
        assert_eq!(
            result.visible_items.last().map(|item| item.index),
            Some(224),
            "measurement should keep the configured trailing retained items"
        );
        assert!(
            result
                .visible_items
                .last()
                .is_some_and(|item| item.offset + item.main_axis_size >= 100.0),
            "visible measurement must fill the viewport before honoring the time budget"
        );
    });
}

#[test]
fn test_time_budgeted_reverse_scroll_does_not_backtrack() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig {
            spacing: 8.0,
            ..Default::default()
        };
        let item_sizes: Vec<f32> = (0..512usize)
            .map(|index| match index % 7 {
                0 => 44.0,
                1 => 60.0,
                2 => 220.0,
                3 => 72.0,
                4 => 96.0,
                5 => 156.0,
                _ => 52.0,
            })
            .collect();

        let mut result =
            measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |index| {
                std::thread::sleep(std::time::Duration::from_millis(55));
                create_test_item(index, item_sizes[index])
            });
        assert_eq!(result.first_visible_item_index, 0);

        for _ in 0..4 {
            state.dispatch_scroll_delta(-320.0);
            result = measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |index| {
                std::thread::sleep(std::time::Duration::from_millis(55));
                create_test_item(index, item_sizes[index])
            });
        }

        assert!(
            result.first_visible_item_index > 0,
            "expected to advance after forward time-budgeted scrolls"
        );

        let mut last_index = result.first_visible_item_index;
        for step in 0..4 {
            state.dispatch_scroll_delta(80.0);
            result = measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |index| {
                std::thread::sleep(std::time::Duration::from_millis(55));
                create_test_item(index, item_sizes[index])
            });
            assert!(
                result.first_visible_item_index <= last_index,
                "reverse time-budgeted step {step} backtracked from index {last_index} to {}",
                result.first_visible_item_index
            );
            last_index = result.first_visible_item_index;
        }
    });
}

#[test]
fn test_backward_scroll_does_not_advance_first_visible_index_for_variable_items() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig {
            spacing: 8.0,
            ..Default::default()
        };
        let item_sizes = [48.0, 56.0, 64.0, 72.0, 80.0];
        let measure_item =
            |index: usize| create_test_item(index, item_sizes[index % item_sizes.len()]);

        let mut result = measure_lazy_list(200, &state, 260.0, 300.0, &config, measure_item);
        assert_eq!(result.first_visible_item_index, 0);

        for _ in 0..28 {
            state.dispatch_scroll_delta(-32.0);
            result = measure_lazy_list(200, &state, 260.0, 300.0, &config, measure_item);
        }

        assert!(
            result.first_visible_item_index >= 12,
            "expected to scroll well into the list before reversing, got index={}",
            result.first_visible_item_index
        );

        let mut last_index = result.first_visible_item_index;
        for step in 0..24 {
            state.dispatch_scroll_delta(12.0);
            result = measure_lazy_list(200, &state, 260.0, 300.0, &config, measure_item);
            assert!(
                result.first_visible_item_index <= last_index,
                "backward step {step} advanced from index {last_index} to {}",
                result.first_visible_item_index
            );
            last_index = result.first_visible_item_index;
        }
    });
}

#[test]
fn test_stored_offset_inside_tall_item_does_not_skip_forward_without_pending_scroll() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(0, 900.0);
        let config = LazyListMeasureConfig {
            spacing: 8.0,
            ..Default::default()
        };
        let item_sizes: Vec<f32> = (0..32usize)
            .map(|index| if index == 0 { 1_200.0 } else { 64.0 })
            .collect();

        let result = measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |i| {
            create_test_item(i, item_sizes[i])
        });

        assert_eq!(
            result.first_visible_item_index, 0,
            "stored in-item offset must not be turned into an average-size forward jump"
        );
        assert!(
            (result.first_visible_item_scroll_offset - 900.0).abs() < 0.01,
            "expected to preserve the stored in-item scroll offset"
        );
    });
}

#[test]
fn test_large_offset_inside_cached_tall_item_does_not_skip_forward_without_forward_scroll() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(20, 900.0);
        let config = LazyListMeasureConfig {
            spacing: 8.0,
            ..Default::default()
        };
        for index in 0..20 {
            state.cache_item_size(index, 60.0 + (index % 3) as f32 * 8.0);
        }
        state.cache_item_size(20, 1_200.0);

        let item_sizes: Vec<f32> = (0..64usize)
            .map(|index| {
                if index == 20 {
                    1_200.0
                } else {
                    60.0 + (index % 3) as f32 * 8.0
                }
            })
            .collect();

        let result = measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |i| {
            create_test_item(i, item_sizes[i])
        });

        assert_eq!(
            result.first_visible_item_index, 20,
            "offset within a tall cached item must not be interpreted as skipping to later average-sized items"
        );
        assert!(
            (result.first_visible_item_scroll_offset - 900.0).abs() < 0.01,
            "expected to preserve in-item offset inside the tall cached item"
        );
    });
}

#[test]
fn test_matches_exact_model_for_variable_item_reverse_scrolls() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig {
            spacing: 8.0,
            ..Default::default()
        };
        let viewport_size = 260.0;
        let item_sizes: Vec<f32> = (0..240usize)
            .map(|index| match index % 9 {
                0 => 32.0,
                1 => 48.0,
                2 => 240.0,
                3 => 56.0,
                4 => 72.0,
                5 => 180.0,
                6 => 40.0,
                7 => 96.0,
                _ => 56.0,
            })
            .collect();
        let deltas = [
            -180.0, -180.0, -220.0, -150.0, -240.0, -120.0, -160.0, 60.0, 60.0, 80.0, -96.0, -96.0,
            44.0, 44.0, 44.0, -140.0, -140.0, 72.0, 72.0, 72.0, 72.0,
        ];
        let expected = exact_scroll_position(&item_sizes, config.spacing, viewport_size, &deltas);

        for (step, (delta, (expected_index, expected_offset))) in
            deltas.iter().zip(expected.iter()).enumerate()
        {
            state.dispatch_scroll_delta(*delta);
            let mut result;
            loop {
                result = measure_lazy_list(
                    item_sizes.len(),
                    &state,
                    viewport_size,
                    320.0,
                    &config,
                    |index| create_test_item(index, item_sizes[index]),
                );
                if state.peek_scroll_delta().abs() <= 0.001 {
                    break;
                }
            }

            assert_eq!(
                result.first_visible_item_index, *expected_index,
                "step {step} delta={delta} expected first index {} but got {}",
                expected_index, result.first_visible_item_index
            );
            assert!(
                (result.first_visible_item_scroll_offset - *expected_offset).abs() < 0.01,
                "step {step} delta={delta} expected offset {:.2} but got {:.2}",
                expected_offset,
                result.first_visible_item_scroll_offset
            );
        }
    });
}

fn assert_end_reachable_with_step(step_dp: f32) {
    with_test_runtime(|| {
        let density = 2.75_f32;
        let viewport = 2280.0 / density;
        let items = 40usize;
        let item_size = 177.0 / density;
        let config = LazyListMeasureConfig::default();
        let state = new_lazy_list_state();

        let mut result = measure_lazy_list(items, &state, viewport, 300.0, &config, |i| {
            create_test_item(i, item_size)
        });

        let mut frames = 0;
        while result.can_scroll_forward && frames < 10_000 {
            state.dispatch_scroll_delta(-step_dp);
            result = measure_lazy_list(items, &state, viewport, 300.0, &config, |i| {
                create_test_item(i, item_size)
            });
            frames += 1;
        }

        assert!(
            !result.can_scroll_forward,
            "list must report the end as reached (step {step_dp} dp)"
        );
        let last = result.visible_items.last().expect("visible items at end");
        assert_eq!(
            last.index,
            items - 1,
            "last item must be reachable (step {step_dp} dp)"
        );
        let last_bottom = last.offset + last.main_axis_size;
        assert!(
            (last_bottom - viewport).abs() < 0.01,
            "last item bottom {last_bottom} must align exactly with the fractional \
             viewport end {viewport} (step {step_dp} dp)"
        );
        assert!(
            result.can_scroll_backward,
            "end position must allow scrolling back"
        );
    });
}

#[test]
fn fractional_density_drag_reaches_exact_end() {
    assert_end_reachable_with_step(15.0);
}

#[test]
fn fractional_density_fling_reaches_exact_end() {
    assert_end_reachable_with_step(128.0);
}

fn drag_to_end_with_tall_items(
    item_sizes: &[f32],
    viewport: f32,
    drag_dp: f32,
) -> (usize, f32, bool) {
    let items = item_sizes.len();
    let config = LazyListMeasureConfig::default();
    let state = new_lazy_list_state();

    let mut result = measure_lazy_list(items, &state, viewport, 300.0, &config, |i| {
        create_test_item(i, item_sizes[i])
    });

    let mut frames = 0;
    while result.can_scroll_forward && frames < 10_000 {
        state.dispatch_scroll_delta(-drag_dp);
        result = measure_lazy_list(items, &state, viewport, 300.0, &config, |i| {
            create_test_item(i, item_sizes[i])
        });
        frames += 1;
    }

    let last = result.visible_items.last().expect("visible items at end");
    let last_bottom = last.offset + last.main_axis_size;
    (last.index, last_bottom, !result.can_scroll_forward)
}

#[test]
fn single_item_taller_than_viewport_scrolls_to_its_bottom() {
    with_test_runtime(|| {
        let viewport = 600.0;
        let (last_index, last_bottom, reached_end) =
            drag_to_end_with_tall_items(&[3000.0], viewport, 280.0);
        assert!(reached_end, "list must eventually report the end");
        assert_eq!(last_index, 0);
        assert!(
            (last_bottom - viewport).abs() < 0.01,
            "single 3000-tall item in a 600 viewport must scroll a full 2400 so its \
             bottom aligns with the viewport end; item bottom ended at {last_bottom}"
        );
    });
}

#[test]
fn trailing_item_taller_than_viewport_scrolls_to_its_bottom() {
    with_test_runtime(|| {
        let viewport = 600.0;
        let (last_index, last_bottom, reached_end) =
            drag_to_end_with_tall_items(&[200.0, 3000.0], viewport, 280.0);
        assert!(reached_end, "list must eventually report the end");
        assert_eq!(last_index, 1);
        assert!(
            (last_bottom - viewport).abs() < 0.01,
            "trailing 3000-tall item must be scrollable until its bottom aligns with \
             the viewport end; item bottom ended at {last_bottom}"
        );
    });
}

#[test]
fn tall_item_scroll_position_advances_within_the_item() {
    with_test_runtime(|| {
        let viewport = 600.0;
        let config = LazyListMeasureConfig::default();
        let state = new_lazy_list_state();
        let sizes = [200.0f32, 3000.0];

        let mut result = measure_lazy_list(2, &state, viewport, 300.0, &config, |i| {
            create_test_item(i, sizes[i])
        });
        let mut consumed_total = 0.0f32;
        for step in 0..20 {
            if !result.can_scroll_forward {
                break;
            }
            let before_index = result.first_visible_item_index;
            let before_offset = result.first_visible_item_scroll_offset;
            state.dispatch_scroll_delta(-280.0);
            result = measure_lazy_list(2, &state, viewport, 300.0, &config, |i| {
                create_test_item(i, sizes[i])
            });
            let advanced = result.first_visible_item_index > before_index
                || result.first_visible_item_scroll_offset > before_offset + 0.001;
            assert!(
                advanced || !result.can_scroll_forward,
                "drag step {step} made no progress: stuck at index {} offset {:.2} while \
                 can_scroll_forward is still true",
                result.first_visible_item_index,
                result.first_visible_item_scroll_offset
            );
            consumed_total += 280.0;
            if consumed_total > 4000.0 {
                break;
            }
        }
        assert!(!result.can_scroll_forward, "end must be reachable");
        assert_eq!(result.first_visible_item_index, 1);
        assert!(
            (result.first_visible_item_scroll_offset - 2400.0).abs() < 0.01,
            "expected final in-item offset 2400 (item bottom at viewport end), got {:.2}",
            result.first_visible_item_scroll_offset
        );
    });
}

#[test]
fn unbounded_viewport_realizes_all_items_and_disables_inner_scroll() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig {
            spacing: 10.0,
            before_content_padding: 4.0,
            after_content_padding: 6.0,
            ..Default::default()
        };
        let sizes = [50.0f32, 800.0, 50.0];

        let result = measure_lazy_list(sizes.len(), &state, f32::INFINITY, 320.0, &config, |i| {
            create_test_item(i, sizes[i])
        });

        assert_eq!(
            result.visible_items.len(),
            sizes.len(),
            "an unbounded viewport must realize every item"
        );
        assert!((result.total_content_size - 930.0).abs() < 0.01);
        assert!((result.viewport_size - 930.0).abs() < 0.01);
        assert!((result.visible_items[0].offset - 4.0).abs() < 0.01);
        assert!((result.visible_items[1].offset - 64.0).abs() < 0.01);
        assert!((result.visible_items[2].offset - 874.0).abs() < 0.01);
        assert!(!result.can_scroll_forward, "outer container owns scrolling");
        assert!(!result.can_scroll_backward);
        assert!(!state.can_scroll_forward_non_reactive());
        assert_eq!(result.first_visible_item_index, 0);
        assert_eq!(state.first_visible_item_index_non_reactive(), 0);
        assert!(state.layout_info().is_infinite_viewport);
    });
}

#[test]
fn leading_content_padding_scrolls_away_without_recycling_visible_item() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig {
            before_content_padding: 100.0,
            after_content_padding: 24.0,
            ..Default::default()
        };
        let measure = |index| create_test_item(index, 50.0);

        let initial = measure_lazy_list(10, &state, 200.0, 300.0, &config, measure);
        assert!((initial.visible_items[0].offset - 100.0).abs() < 0.01);

        state.dispatch_scroll_delta(-120.0);
        let partial = measure_lazy_list(10, &state, 200.0, 300.0, &config, measure);
        let item0 = partial
            .visible_items
            .iter()
            .find(|item| item.index == 0)
            .expect("partially visible first item must remain measured");
        assert!(
            (item0.offset + 20.0).abs() < 0.01,
            "offset={}",
            item0.offset
        );
        assert_eq!(partial.first_visible_item_index, 0);
        assert!((partial.first_visible_item_scroll_offset - 120.0).abs() < 0.01);
        assert_eq!(state.layout_info().viewport_start_offset, 0.0);
        assert_eq!(state.layout_info().viewport_end_offset, 200.0);

        state.dispatch_scroll_delta(-31.0);
        let gone = measure_lazy_list(10, &state, 200.0, 300.0, &config, measure);
        assert_eq!(gone.first_visible_item_index, 1);
        assert!(
            state
                .layout_info()
                .visible_items_info
                .iter()
                .all(|item| item.index != 0),
            "fully clipped item 0 may be retained for prewarm but must not be reported visible"
        );
        assert!((gone.first_visible_item_scroll_offset - 1.0).abs() < 0.01);
    });
}
