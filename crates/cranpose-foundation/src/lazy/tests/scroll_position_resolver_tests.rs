use super::{
    super::lazy_list_state::test_helpers::{
        new_lazy_list_state, new_lazy_list_state_with_position, with_test_runtime,
    },
    *,
};

#[test]
fn test_apply_pending_scroll_delta_from_default_state() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        let config = LazyListMeasureConfig::default();
        let resolver =
            ScrollPositionResolver::new(&state, state.begin_measure_pass(), &config, 100, 500.0);

        let (index, offset) = resolver.apply_pending_scroll_delta();
        assert_eq!(index, 0);
        assert_eq!(offset, 0.0);
    });
}

#[test]
fn test_apply_pending_scroll_delta_with_initial_position() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(5, 25.0);
        let config = LazyListMeasureConfig::default();
        let resolver =
            ScrollPositionResolver::new(&state, state.begin_measure_pass(), &config, 100, 500.0);

        let (index, offset) = resolver.apply_pending_scroll_delta();
        assert_eq!(index, 5);
        assert_eq!(offset, 25.0);
    });
}

#[test]
fn test_apply_pending_scroll_delta_clamps_beyond_items_count() {
    with_test_runtime(|| {
        let state = new_lazy_list_state_with_position(50, 0.0);
        let config = LazyListMeasureConfig::default();
        let resolver =
            ScrollPositionResolver::new(&state, state.begin_measure_pass(), &config, 10, 500.0);

        let (index, _offset) = resolver.apply_pending_scroll_delta();
        assert_eq!(index, 9);
    });
}

#[test]
fn test_scroll_to_request() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        state.scroll_to_item(10, 15.0);
        let config = LazyListMeasureConfig::default();
        let resolver =
            ScrollPositionResolver::new(&state, state.begin_measure_pass(), &config, 100, 500.0);

        let (index, offset) = resolver.apply_pending_scroll_delta();
        assert_eq!(index, 10);
        assert_eq!(offset, 15.0);
    });
}

#[test]
fn test_normalize_forward_skips_items() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        state.cache_item_size(0, 100.0);
        let config = LazyListMeasureConfig::default();
        let resolver =
            ScrollPositionResolver::new(&state, state.begin_measure_pass(), &config, 100, 500.0);

        let (index, offset) = resolver.normalize_forward(0, 1500.0);
        assert!(index > 0, "Expected forward jump, got index={index}");
        assert!(offset < 1500.0, "Expected offset reduction");
    });
}

#[test]
fn test_normalize_forward_with_cache_preserves_offset_inside_tall_item() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        for (index, size) in [48.0, 56.0, 64.0, 72.0, 80.0].into_iter().enumerate() {
            state.cache_item_size(index, size);
        }
        state.cache_item_size(5, 1_200.0);
        let config = LazyListMeasureConfig {
            spacing: 8.0,
            ..Default::default()
        };
        let resolver =
            ScrollPositionResolver::new(&state, state.begin_measure_pass(), &config, 32, 260.0);

        let (index, offset) = resolver.normalize_forward_with_cache(5, 900.0);

        assert_eq!(index, 5);
        assert!((offset - 900.0).abs() < 0.001);
    });
}

#[test]
fn test_normalize_backward_jump_reduces_offset() {
    with_test_runtime(|| {
        let state = new_lazy_list_state();
        state.cache_item_size(0, 100.0);
        let config = LazyListMeasureConfig::default();
        let resolver =
            ScrollPositionResolver::new(&state, state.begin_measure_pass(), &config, 100, 500.0);

        let (index, offset) = resolver.normalize_backward_jump(50, -2000.0);
        assert!(index < 50, "Expected backward jump, got index={index}");
        assert!(offset > -2000.0, "Expected offset increase");
    });
}
