use cranpose_core::{Composition, MemoryApplier, location_key};

use super::*;

#[test]
fn test_lazy_column_spec_default() {
    let spec = LazyColumnSpec::default();
    assert_eq!(spec.vertical_arrangement, LinearArrangement::Start);
    assert_eq!(spec.beyond_bounds_item_count, 2);
}

#[test]
fn test_lazy_row_spec_default() {
    let spec = LazyRowSpec::default();
    assert_eq!(spec.horizontal_arrangement, LinearArrangement::Start);
    assert_eq!(spec.beyond_bounds_item_count, 2);
}

/// A 20 by 50 item at `index`, placed at `offset`, with roots `node_ids` at
/// `child_offsets`.
fn placed_item(
    index: usize,
    offset: f32,
    node_ids: &[u64],
    child_offsets: &[f32],
) -> LazyListMeasuredItem {
    let roots = LazyItemRoots {
        node_ids: SmallVec::from_slice(node_ids),
        child_offsets: SmallVec::from_slice(child_offsets),
        child_cross_axis_sizes: SmallVec::new(),
        measurements: SmallVec::new(),
    };
    let mut item =
        LazyListMeasuredItem::new(index, 10 + index as u64, None, 20.0, 50.0, Rc::new(roots));
    item.offset = offset;
    item
}

#[test]
fn lazy_list_placements_reuse_output_storage() {
    let item = placed_item(0, 7.0, &[101, 102], &[0.0, 5.0]);
    let config = LazyListLayoutConfig {
        measure: LazyListMeasureConfig {
            is_vertical: true,
            is_rtl: false,
            reverse_layout: false,
            before_content_padding: 0.0,
            after_content_padding: 0.0,
            spacing: 0.0,
            beyond_bounds_item_count: 0,
            vertical_arrangement: Some(LinearArrangement::Start),
            horizontal_arrangement: None,
        },
        cross_padding: (0.0, 0.0),
        grid: None,
    };
    let mut placements = Vec::with_capacity(8);
    let original_capacity = placements.capacity();

    push_lazy_list_placements(&mut placements, 1.0, &[item], 1, true, 100.0, &config, 50.0);

    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].node_id, 101);
    assert_eq!(placements[0].y, 7.0);
    assert_eq!(placements[1].node_id, 102);
    assert_eq!(placements[1].y, 12.0);
    assert_eq!(placements.capacity(), original_capacity);
}

#[test]
fn lazy_list_placements_retain_offscreen_measured_items_for_renderer_prewarm() {
    let hidden = placed_item(0, -40.0, &[101], &[0.0]);
    let partial = placed_item(1, -5.0, &[102], &[0.0]);

    let config = LazyListLayoutConfig {
        measure: LazyListMeasureConfig {
            is_vertical: true,
            is_rtl: false,
            reverse_layout: false,
            before_content_padding: 0.0,
            after_content_padding: 0.0,
            spacing: 0.0,
            beyond_bounds_item_count: 2,
            vertical_arrangement: Some(LinearArrangement::Start),
            horizontal_arrangement: None,
        },
        cross_padding: (0.0, 0.0),
        grid: None,
    };
    let mut placements = Vec::new();

    push_lazy_list_placements(
        &mut placements,
        1.0,
        &[hidden, partial],
        100,
        true,
        100.0,
        &config,
        50.0,
    );

    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].node_id, 101);
    assert_eq!(placements[0].y, -40.0);
    assert_eq!(placements[1].node_id, 102);
    assert_eq!(placements[1].y, -5.0);
}

#[test]
fn lazy_measure_policy_does_not_schedule_speculative_prefetch_frames() {
    let source = include_str!("../lazy_list.rs");
    let start = source
        .find("fn measure_lazy_list_internal")
        .expect("measure function exists");
    let end = source[start..]
        .find("fn bind_layout_invalidation_callback")
        .map(|offset| start + offset)
        .expect("measure function boundary exists");
    let body = &source[start..end];

    assert!(
        !body.contains("prefetch_lazy_list_items")
            && !body.contains("schedule_layout_prewarm_repass"),
        "lazy layout measurement must not schedule speculative frame work"
    );
}

#[test]
fn active_scroll_cached_reuse_validates_retained_children() {
    let source = include_str!("../lazy_list.rs");
    let trust_mode = ["TrustClean", "RetainedScrollItem"].concat();
    let trust_api = ["trusting_", "cached_children"].concat();

    assert!(
        !source.contains(trust_mode.as_str()) && !source.contains(trust_api.as_str()),
        "lazy cached reuse must validate retained children during active scroll"
    );
}

#[test]
fn lazy_list_state_identity_is_stable_for_copied_state() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let mut state = None;
    composition
        .render(key, || {
            state = Some(cranpose_foundation::lazy::rememberLazyListState());
        })
        .expect("lazy list state render should succeed");
    let state = state.expect("lazy list state should be captured");
    let copied_state = state;

    assert_ne!(state.inner_ptr(), std::ptr::null());
    assert_eq!(
        lazy_list_state_identity(&state),
        lazy_list_state_identity(&copied_state)
    );
}

#[test]
fn measured_item_cache_keeps_only_items_whose_slots_are_retained() {
    let mut cache = LazyMeasuredItemCache::default();
    for key in 1..=6u64 {
        cache.insert(
            LazyListMeasuredItem::new(key as usize, key, None, 10.0, 10.0, Rc::default()),
            10.0,
        );
    }

    cache.retain_retained_slots(|slot| slot.raw() % 2 == 0);

    let mut kept: Vec<u64> = cache
        .entries
        .values()
        .map(|cached| cached.item.key)
        .collect();
    kept.sort_unstable();
    assert_eq!(kept, [2, 4, 6], "disposed items' measurements are dropped");
}

#[test]
fn a_frame_composes_only_the_new_items_its_scroll_reaches_where_idle_passes_run() {
    let mut frame = BeyondBoundsComposition::new(false, 6.0);
    assert!(
        frame.admits(false, 4.0),
        "an item the scroll reaches next frame"
    );
    assert!(!frame.admits(false, 4.0 + 120.0));
    assert!(frame.wants_prefetch, "the farther item waits for a pass");

    let mut at_rest = BeyondBoundsComposition::new(false, 0.0);
    assert!(at_rest.admits(true, 0.0), "a composed item stays composed");
    assert!(!at_rest.admits(false, 0.0));
    assert!(at_rest.wants_prefetch);
}

#[test]
fn a_frame_leaves_the_next_new_item_for_an_idle_pass_once_enough_are_ready() {
    let mut frame = BeyondBoundsComposition::new(false, f32::INFINITY);
    for _ in 0..IDLE_PREFETCH_READY_BEYOND_BOUNDS_ITEMS {
        assert!(frame.admits(true, 0.0));
    }
    assert!(!frame.wants_prefetch);
    assert!(!frame.admits(false, 0.0));
    assert!(frame.wants_prefetch);
}

#[test]
fn a_frame_composes_new_items_only_until_enough_are_ready() {
    let mut frame = BeyondBoundsComposition::new(false, f32::INFINITY);
    assert!(frame.admits(true, 0.0));
    for _ in 1..IDLE_PREFETCH_READY_BEYOND_BOUNDS_ITEMS {
        assert!(frame.admits(false, 0.0));
    }
    assert!(
        !frame.admits(false, 0.0),
        "the items a frame composes count as ready: a wider window gives it \
         no more to compose"
    );
    assert!(frame.wants_prefetch, "what the frame left waits for a pass");
}

#[test]
fn a_frame_with_none_ready_composes_as_many_as_it_keeps_ready() {
    let mut frame = BeyondBoundsComposition::new(false, f32::INFINITY);
    for _ in 0..IDLE_PREFETCH_READY_BEYOND_BOUNDS_ITEMS {
        assert!(frame.admits(false, 0.0));
    }
    assert!(!frame.admits(false, 0.0));
    assert!(frame.wants_prefetch);
}

#[test]
fn an_idle_pass_composes_one_new_item_and_asks_for_the_next() {
    let mut idle = BeyondBoundsComposition::new(true, f32::INFINITY);
    for _ in 0..3 {
        assert!(idle.admits(true, 0.0));
    }
    assert!(idle.admits(false, 0.0));
    assert!(!idle.wants_prefetch);
    assert!(!idle.admits(false, 0.0));
    assert!(idle.wants_prefetch);
}
