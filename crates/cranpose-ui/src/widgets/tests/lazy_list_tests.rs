use cranpose_core::{Composition, MemoryApplier, location_key};

use super::*;

#[test]
fn test_lazy_column_spec_default() {
    let spec = LazyColumnSpec::default();
    assert_eq!(spec.vertical_arrangement, LinearArrangement::Start);
    assert_eq!(spec.beyond_bounds_item_count, 2);
}

#[test]
fn test_lazy_column_spec_builder() {
    let spec = LazyColumnSpec::new()
        .vertical_arrangement(LinearArrangement::SpacedBy(8.0))
        .content_padding(16.0, 16.0);

    assert_eq!(spec.vertical_arrangement, LinearArrangement::SpacedBy(8.0));
    assert_eq!(spec.content_padding_top, 16.0);
}

#[test]
fn test_lazy_row_spec_default() {
    let spec = LazyRowSpec::default();
    assert_eq!(spec.horizontal_arrangement, LinearArrangement::Start);
    assert_eq!(spec.beyond_bounds_item_count, 2);
}

#[test]
fn test_get_spacing() {
    assert_eq!(get_spacing(LinearArrangement::Start), 0.0);
    assert_eq!(get_spacing(LinearArrangement::SpacedBy(12.0)), 12.0);
}

#[test]
fn test_content_padding_all() {
    let spec = LazyColumnSpec::new().content_padding_all(24.0);
    assert_eq!(spec.content_padding_top, 24.0);
    assert_eq!(spec.content_padding_bottom, 24.0);
}

#[test]
fn lazy_list_placements_reuse_output_storage() {
    let mut item = LazyListMeasuredItem::new(0, 10, None, 20.0, 50.0);
    item.offset = 7.0;
    item.node_ids.push(101);
    item.node_ids.push(102);
    item.child_offsets.push(0.0);
    item.child_offsets.push(5.0);
    let config = LazyListMeasureConfig {
        is_vertical: true,
        reverse_layout: false,
        before_content_padding: 0.0,
        after_content_padding: 0.0,
        spacing: 0.0,
        beyond_bounds_item_count: 0,
        vertical_arrangement: Some(LinearArrangement::Start),
        horizontal_arrangement: None,
    };
    let mut placements = Vec::with_capacity(8);
    let original_capacity = placements.capacity();

    push_lazy_list_placements(&mut placements, &[item], 1, true, 100.0, &config);

    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].node_id, 101);
    assert_eq!(placements[0].y, 7.0);
    assert_eq!(placements[1].node_id, 102);
    assert_eq!(placements[1].y, 12.0);
    assert_eq!(placements.capacity(), original_capacity);
}

#[test]
fn lazy_list_placements_retain_offscreen_measured_items_for_renderer_prewarm() {
    let mut hidden = LazyListMeasuredItem::new(0, 10, None, 20.0, 50.0);
    hidden.offset = -40.0;
    hidden.node_ids.push(101);
    hidden.child_offsets.push(0.0);

    let mut partial = LazyListMeasuredItem::new(1, 11, None, 20.0, 50.0);
    partial.offset = -5.0;
    partial.node_ids.push(102);
    partial.child_offsets.push(0.0);

    let config = LazyListMeasureConfig {
        is_vertical: true,
        reverse_layout: false,
        before_content_padding: 0.0,
        after_content_padding: 0.0,
        spacing: 0.0,
        beyond_bounds_item_count: 2,
        vertical_arrangement: Some(LinearArrangement::Start),
        horizontal_arrangement: None,
    };
    let mut placements = Vec::new();

    push_lazy_list_placements(
        &mut placements,
        &[hidden, partial],
        100,
        true,
        100.0,
        &config,
    );

    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].node_id, 101);
    assert_eq!(placements[0].y, -40.0);
    assert_eq!(placements[1].node_id, 102);
    assert_eq!(placements[1].y, -5.0);
}

#[test]
fn lazy_list_placements_retain_after_viewport_prefetch_items_for_renderer_prewarm() {
    let mut visible = LazyListMeasuredItem::new(0, 10, None, 40.0, 50.0);
    visible.offset = 60.0;
    visible.node_ids.push(101);
    visible.child_offsets.push(0.0);

    let mut warm = LazyListMeasuredItem::new(1, 11, None, 40.0, 50.0);
    warm.offset = 110.0;
    warm.node_ids.push(102);
    warm.child_offsets.push(0.0);

    let mut far = LazyListMeasuredItem::new(2, 12, None, 40.0, 50.0);
    far.offset = 158.0;
    far.node_ids.push(103);
    far.child_offsets.push(0.0);

    let config = LazyListMeasureConfig {
        is_vertical: true,
        reverse_layout: false,
        before_content_padding: 0.0,
        after_content_padding: 0.0,
        spacing: 8.0,
        beyond_bounds_item_count: 8,
        vertical_arrangement: Some(LinearArrangement::SpacedBy(8.0)),
        horizontal_arrangement: None,
    };
    let mut placements = Vec::new();

    push_lazy_list_placements(
        &mut placements,
        &[visible, warm, far],
        100,
        true,
        100.0,
        &config,
    );

    let placed_nodes = placements.iter().map(|p| p.node_id).collect::<Vec<_>>();
    assert_eq!(
        placed_nodes,
        vec![101, 102, 103],
        "prefetch rows remain in the retained placement list so renderers can prewarm clipped content"
    );
}

#[test]
fn lazy_measure_policy_does_not_schedule_speculative_prefetch_frames() {
    let source = include_str!("../lazy_list.rs");
    let start = source
        .find("fn measure_lazy_list_internal")
        .expect("measure function exists");
    let end = source[start..]
        .find("fn get_spacing")
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
