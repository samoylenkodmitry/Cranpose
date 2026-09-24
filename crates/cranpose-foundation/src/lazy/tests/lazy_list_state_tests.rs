use std::{cell::Cell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};

use super::{
    LazyListItemInfo, LazyListLayoutInfo, LazyListState,
    test_helpers::{new_lazy_list_state, new_lazy_list_state_with_position, with_test_runtime},
};

fn set_scroll_bounds(state: &LazyListState, can_forward: bool, can_backward: bool) {
    state.can_scroll_forward_state.set(can_forward);
    state.can_scroll_backward_state.set(can_backward);
    state.inner.with(|rc| {
        let mut inner = rc.borrow_mut();
        inner.current_can_scroll_forward = can_forward;
        inner.current_can_scroll_backward = can_backward;
    });
}

fn enable_bidirectional_scroll(state: &LazyListState) {
    set_scroll_bounds(state, true, true);
}

fn mark_scroll_bounds_known(state: &LazyListState) {
    state.update_layout_info(LazyListLayoutInfo {
        total_items_count: 10,
        ..Default::default()
    });
}

fn visible_item(index: usize, offset: f32, size: f32) -> LazyListItemInfo {
    LazyListItemInfo {
        index,
        key: index as u64,
        offset,
        size,
    }
}

#[test]
fn lazy_measure_telemetry_ids_are_state_owned() {
    with_test_runtime(|| {
        let first = new_lazy_list_state();
        let second = new_lazy_list_state();

        assert_eq!(first.next_measure_cycle_id(), 1);
        assert_eq!(first.next_measure_cycle_id(), 2);
        assert_eq!(second.next_measure_cycle_id(), 1);

        assert_eq!(first.next_item_measure_pass_id(), 1);
        assert_eq!(first.next_item_measure_pass_id(), 2);
        assert_eq!(second.next_item_measure_pass_id(), 1);
    });
}

#[test]
fn measure_result_updates_retained_and_reactive_scroll_position() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();

        state.update_scroll_position_with_key(8, 17.5, 123);

        assert_eq!(state.scroll_position.index.get_non_reactive(), 8);
        assert!((state.scroll_position.scroll_offset.get_non_reactive() - 17.5).abs() < 0.001);
        assert_eq!(state.first_visible_item_index_non_reactive(), 8);
        assert!((state.first_visible_item_scroll_offset_non_reactive() - 17.5).abs() < 0.001);
    });
}

#[test]
fn update_scroll_bounds_updates_retained_and_reactive_capabilities() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();

        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: vec![visible_item(0, 0.0, 40.0), visible_item(1, 40.0, 40.0)],
            total_items_count: 10,
            viewport_size: 80.0,
            ..Default::default()
        });
        state.update_scroll_bounds();

        assert!(state.can_scroll_forward_state.get_non_reactive());
        assert!(!state.can_scroll_backward_state.get_non_reactive());
        assert!(state.can_scroll_forward_non_reactive());
        assert!(!state.can_scroll_backward_non_reactive());

        state.update_scroll_position(3, 2.0);
        state.update_scroll_bounds();

        assert!(state.can_scroll_backward_state.get_non_reactive());
        assert!(state.can_scroll_backward_non_reactive());
    });
}

#[test]
fn layout_info_snap_anchor_tracks_common_item_offset_delta() {
    let previous = LazyListLayoutInfo {
        visible_items_info: vec![visible_item(15, -31.4, 30.0), visible_item(16, 4.6, 30.0)],
        snap_anchor_offset: -31.4,
        ..Default::default()
    };
    let current = LazyListLayoutInfo {
        visible_items_info: vec![visible_item(16, 3.6, 30.0), visible_item(17, 39.6, 30.0)],
        ..Default::default()
    };

    let anchor = super::continuous_snap_anchor_offset(&previous, &current);

    assert!((anchor + 32.4).abs() <= 0.001);
}

#[test]
fn layout_info_snap_anchor_uses_reverse_visual_item_offset() {
    let previous = LazyListLayoutInfo {
        visible_items_info: vec![visible_item(15, 31.4, 30.0), visible_item(16, 67.4, 30.0)],
        snap_anchor_offset: 58.6,
        viewport_size: 120.0,
        reverse_layout: true,
        ..Default::default()
    };
    let current = LazyListLayoutInfo {
        visible_items_info: vec![visible_item(16, 68.4, 30.0), visible_item(17, 104.4, 30.0)],
        viewport_size: 120.0,
        reverse_layout: true,
        ..Default::default()
    };

    let anchor = super::continuous_snap_anchor_offset(&previous, &current);

    assert!((anchor - 57.6).abs() <= 0.001);
}

#[test]
fn update_layout_info_keeps_snap_anchor_continuous_when_first_visible_item_changes() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: vec![visible_item(15, -31.4, 30.0), visible_item(16, 4.6, 30.0)],
            ..Default::default()
        });

        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: vec![visible_item(16, 3.6, 30.0), visible_item(17, 39.6, 30.0)],
            ..Default::default()
        });

        let info = state.layout_info();
        assert!((info.snap_anchor_offset + 32.4).abs() <= 0.001);
    });
}

#[test]
fn dispatch_scroll_delta_accumulates_same_direction() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        enable_bidirectional_scroll(&state);

        state.dispatch_scroll_delta(-12.0);
        state.dispatch_scroll_delta(-8.0);

        assert!((state.peek_scroll_delta() + 20.0).abs() < 0.001);
        let snapshot = state.begin_measure_pass();
        assert!((snapshot.pending_scroll_delta + 20.0).abs() < 0.001);
        assert_eq!(state.begin_measure_pass().pending_scroll_delta, 0.0);
    });
}

#[test]
fn dispatch_scroll_delta_drops_stale_backlog_on_direction_change() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        enable_bidirectional_scroll(&state);

        state.dispatch_scroll_delta(-120.0);
        state.dispatch_scroll_delta(-30.0);
        assert!((state.peek_scroll_delta() + 150.0).abs() < 0.001);

        state.dispatch_scroll_delta(18.0);

        assert!((state.peek_scroll_delta() - 18.0).abs() < 0.001);
        let snapshot = state.begin_measure_pass();
        assert!((snapshot.pending_scroll_delta - 18.0).abs() < 0.001);
        assert_eq!(state.begin_measure_pass().pending_scroll_delta, 0.0);
    });
}

#[test]
fn dispatch_scroll_delta_clamps_pending_backlog() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        enable_bidirectional_scroll(&state);

        state.dispatch_scroll_delta(-1_500.0);
        state.dispatch_scroll_delta(-1_500.0);
        assert!((state.peek_scroll_delta() + super::MAX_PENDING_SCROLL_DELTA).abs() < 0.001);

        state.dispatch_scroll_delta(3_000.0);
        assert!((state.peek_scroll_delta() - super::MAX_PENDING_SCROLL_DELTA).abs() < 0.001);
    });
}

#[test]
fn begin_measure_pass_consumes_large_pending_scroll_delta_coherently() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        enable_bidirectional_scroll(&state);
        let invalidations = Rc::new(Cell::new(0u32));
        let invalidations_clone = Rc::clone(&invalidations);
        state.add_invalidate_callback(Rc::new(move || {
            invalidations_clone.set(invalidations_clone.get() + 1);
        }));

        state.dispatch_scroll_delta(-1_000.0);
        assert!((state.peek_scroll_delta() + 1_000.0).abs() < 0.001);

        let first = state.begin_measure_pass();
        assert!(
            (first.pending_scroll_delta + 1_000.0).abs() < 0.001,
            "first pass should consume the whole coherent scroll input"
        );
        assert!(
            state.peek_scroll_delta().abs() < 0.001,
            "measure pass should not retain a synthetic scroll backlog"
        );
        assert_eq!(
            invalidations.get(),
            1,
            "dispatch should request layout once; consuming scroll should not schedule follow-up frames"
        );

        let second = state.begin_measure_pass();
        assert!(
            second.pending_scroll_delta.abs() < 0.001,
            "second pass should not receive synthetic remainder"
        );
    });
}

#[test]
fn dispatch_scroll_delta_skips_invalidate_when_clamped_value_is_unchanged() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        enable_bidirectional_scroll(&state);
        let invalidations = Rc::new(Cell::new(0u32));
        let invalidations_clone = Rc::clone(&invalidations);
        state.add_invalidate_callback(Rc::new(move || {
            invalidations_clone.set(invalidations_clone.get() + 1);
        }));

        state.dispatch_scroll_delta(-3_000.0);
        assert_eq!(invalidations.get(), 1);
        assert!((state.peek_scroll_delta() + super::MAX_PENDING_SCROLL_DELTA).abs() < 0.001);

        state.dispatch_scroll_delta(-100.0);
        assert_eq!(invalidations.get(), 1);

        state.dispatch_scroll_delta(100.0);
        assert_eq!(invalidations.get(), 2);
    });
}

#[test]
fn begin_measure_pass_takes_coherent_snapshot_and_consumes_pending_inputs() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(3, 12.0);
        state.dispatch_scroll_delta(-20.0);
        state.inner.with(|rc| {
            rc.borrow_mut().pending_scroll_to_index = Some((8, 4.0));
        });

        let snapshot = state.begin_measure_pass();

        assert_eq!(snapshot.first_visible_item_index, 3);
        assert!((snapshot.first_visible_item_scroll_offset - 12.0).abs() < 0.001);
        assert!((snapshot.pending_scroll_delta + 20.0).abs() < 0.001);
        assert_eq!(snapshot.pending_scroll_to, Some((8, 4.0)));
        assert_eq!(state.peek_scroll_delta(), 0.0);
        assert_eq!(state.begin_measure_pass().pending_scroll_to, None);
    });
}

#[test]
fn item_size_cache_refresh_keeps_recent_entry_and_evicts_oldest_live_entry() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        for index in 0..super::ITEM_SIZE_CACHE_CAPACITY {
            state.cache_item_size(index, index as f32 + 10.0);
        }

        state.cache_item_size(0, 999.0);
        state.cache_item_size(super::ITEM_SIZE_CACHE_CAPACITY, 123.0);

        assert_eq!(state.get_cached_size(0), Some(999.0));
        assert_eq!(state.get_cached_size(1), None);
        assert_eq!(
            state.get_cached_size(super::ITEM_SIZE_CACHE_CAPACITY),
            Some(123.0),
        );
    });
}

#[test]
fn item_size_cache_read_promotes_entry_for_large_scroll_reuse() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        for index in 0..super::ITEM_SIZE_CACHE_CAPACITY {
            state.cache_item_size(index, index as f32 + 10.0);
        }

        assert_eq!(state.get_cached_size(0), Some(10.0));
        state.cache_item_size(super::ITEM_SIZE_CACHE_CAPACITY, 123.0);

        assert_eq!(state.get_cached_size(0), Some(10.0));
        assert_eq!(state.get_cached_size(1), None);
        let cache_len = state
            .inner
            .try_with(|rc| rc.borrow().item_size_cache.len())
            .unwrap_or(0);
        assert_eq!(cache_len, super::ITEM_SIZE_CACHE_CAPACITY);
    });
}

#[test]
fn item_size_cache_promotion_queue_stays_bounded_under_hot_reuse() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        state.cache_item_size(0, 32.0);

        for _ in 0..super::ITEM_SIZE_CACHE_CAPACITY * 8 {
            assert_eq!(state.get_cached_size(0), Some(32.0));
        }

        let (cache_len, queue_len) = state
            .inner
            .try_with(|rc| {
                let inner = rc.borrow();
                (
                    inner.item_size_cache.len(),
                    inner.item_size_eviction_queue.len(),
                )
            })
            .unwrap_or((0, 0));
        assert_eq!(cache_len, 1);
        assert!(
            queue_len <= super::ITEM_SIZE_CACHE_CAPACITY,
            "stale promotion tickets must be compacted, got {queue_len}"
        );
    });
}

#[test]
fn cache_item_sizes_updates_average_only_for_new_entries() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();

        let average = state.cache_item_sizes([(0, 10.0), (1, 20.0), (0, 12.0)]);

        assert_eq!(state.get_cached_size(0), Some(12.0));
        assert_eq!(state.get_cached_size(1), Some(20.0));
        assert!((average - 15.0).abs() < 0.001);
    });
}

#[test]
fn layout_callback_can_be_registered_again_after_removal() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let first_node: cranpose_core::NodeId = 1;
        let second_node: cranpose_core::NodeId = 2;

        let first_id = state
            .try_register_layout_callback(first_node, Rc::new(|| {}))
            .expect("first layout callback should register");
        let duplicate_id = state
            .try_register_layout_callback(first_node, Rc::new(|| {}))
            .expect("duplicate register should replace with a fresh callback id");
        assert_eq!(
            state
                .inner
                .with(|rc| rc.borrow().layout_invalidation_callback_id),
            Some(duplicate_id),
            "duplicate registration should become the active callback",
        );
        assert_ne!(
            first_id, duplicate_id,
            "duplicate registration should replace the old callback id",
        );

        state.remove_invalidate_callback(first_id);

        let second_id = state
            .try_register_layout_callback(second_node, Rc::new(|| {}))
            .expect("layout callback should register again after removal");
        assert_ne!(first_id, second_id);
    });
}

#[test]
fn layout_callback_rebinds_when_node_id_changes() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let first_node: cranpose_core::NodeId = 11;
        let second_node: cranpose_core::NodeId = 22;

        let first_id = state
            .try_register_layout_callback(first_node, Rc::new(|| {}))
            .expect("first layout callback should register");

        let second_id = state
            .try_register_layout_callback(second_node, Rc::new(|| {}))
            .expect("layout callback should rebind to a new node");

        assert_ne!(first_id, second_id);
    });
}

#[test]
fn stale_layout_callback_disposer_cannot_remove_replaced_same_node_callback() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let node_id: cranpose_core::NodeId = 7;
        let first_hits = Rc::new(Cell::new(0u32));
        let second_hits = Rc::new(Cell::new(0u32));

        let first_id = state
            .try_register_layout_callback(
                node_id,
                Rc::new({
                    let first_hits = Rc::clone(&first_hits);
                    move || first_hits.set(first_hits.get() + 1)
                }),
            )
            .expect("first layout callback should register");

        let second_id = state
            .try_register_layout_callback(
                node_id,
                Rc::new({
                    let second_hits = Rc::clone(&second_hits);
                    move || second_hits.set(second_hits.get() + 1)
                }),
            )
            .expect("same-node registration should replace the active callback");

        assert_ne!(first_id, second_id);

        state.remove_invalidate_callback(first_id);
        state.dispatch_scroll_delta(-12.0);

        assert_eq!(
            first_hits.get(),
            0,
            "replaced callback should not be invoked after removal",
        );
        assert_eq!(
            second_hits.get(),
            1,
            "active callback should survive stale disposer cleanup",
        );
    });
}

#[test]
fn dispatch_scroll_delta_returns_zero_when_forward_is_blocked() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        mark_scroll_bounds_known(&state);
        set_scroll_bounds(&state, false, true);

        let consumed = state.dispatch_scroll_delta(-24.0);

        assert_eq!(consumed, 0.0);
        assert_eq!(state.peek_scroll_delta(), 0.0);
    });
}

#[test]
fn equality_does_not_deref_released_inner_state() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let mut first = None;
    composition
        .render(key, || {
            first = Some(super::rememberLazyListState());
        })
        .expect("initial render");
    let first = first.expect("first lazy state");

    composition
        .render(key, || {})
        .expect("dispose first lazy state");
    assert!(
        !first.inner.is_alive(),
        "expected first lazy state to be released after disposal"
    );

    let mut second = None;
    composition
        .render(key, || {
            second = Some(super::rememberLazyListState());
        })
        .expect("second render");
    let second = second.expect("second lazy state");

    assert!(
        first != second,
        "released lazy state handle must compare by identity without panicking"
    );
}

#[test]
fn released_lazy_list_state_scroll_position_methods_do_not_panic() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let mut released = None;
    composition
        .render(key, || {
            released = Some(super::rememberLazyListState());
        })
        .expect("initial render");
    let released = released.expect("lazy list state");

    composition
        .render(key, || {})
        .expect("dispose lazy list state");
    assert!(
        !released.inner.is_alive(),
        "expected lazy list state to be released after disposal"
    );

    assert_eq!(released.first_visible_item_index(), 0);
    assert_eq!(released.first_visible_item_scroll_offset(), 0.0);
    assert_eq!(released.nearest_range(), 0..0);
    assert_eq!(
        released.update_scroll_position_if_item_moved(10, |_| Some(0)),
        0
    );
    released.update_scroll_position(3, 12.0);
    released.update_scroll_position_with_key(3, 12.0, 42);
    released.update_scroll_bounds();
}

#[test]
fn dispatch_scroll_delta_clears_stale_pending_at_forward_edge() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        mark_scroll_bounds_known(&state);
        enable_bidirectional_scroll(&state);
        state.dispatch_scroll_delta(-300.0);
        assert!((state.peek_scroll_delta() + 300.0).abs() < 0.001);

        set_scroll_bounds(&state, false, true);

        let blocked_consumed = state.dispatch_scroll_delta(-10.0);
        assert_eq!(blocked_consumed, 0.0);
        assert_eq!(state.peek_scroll_delta(), 0.0);

        let reverse_consumed = state.dispatch_scroll_delta(12.0);
        assert_eq!(reverse_consumed, 12.0);
        assert!((state.peek_scroll_delta() - 12.0).abs() < 0.001);
    });
}

#[test]
fn negative_scroll_delta_prefetches_forward_items() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        state.dispatch_scroll_delta(-24.0);
        state.record_scroll_direction(state.peek_scroll_delta());
        state.update_prefetch_queue(10, 15, 100);

        assert_eq!(state.take_prefetch_indices(), vec![16, 17]);
    });
}
