use super::{
    lazy_list_measure::LazyListMeasureConfig,
    lazy_list_state::{LazyListMeasureStateSnapshot, LazyListState},
};

pub struct ScrollPositionResolver<'a> {
    state: &'a LazyListState,
    measure_state: LazyListMeasureStateSnapshot,
    config: &'a LazyListMeasureConfig,
    items_count: usize,
    effective_viewport_size: f32,
}

impl<'a> ScrollPositionResolver<'a> {
    pub fn new(
        state: &'a LazyListState,
        measure_state: LazyListMeasureStateSnapshot,
        config: &'a LazyListMeasureConfig,
        items_count: usize,
        effective_viewport_size: f32,
    ) -> Self {
        Self {
            state,
            measure_state,
            config,
            items_count,
            effective_viewport_size,
        }
    }

    pub(crate) fn apply_pending_scroll_delta(&self) -> (usize, f32) {
        let (index, mut offset) = self.get_initial_position();
        offset -= self.measure_state.pending_scroll_delta;
        (index, offset)
    }

    fn get_initial_position(&self) -> (usize, f32) {
        if let Some((target_index, target_offset)) = self.measure_state.pending_scroll_to {
            let clamped = target_index.min(self.items_count.saturating_sub(1));
            (clamped, target_offset)
        } else {
            (
                self.measure_state
                    .first_visible_item_index
                    .min(self.items_count.saturating_sub(1)),
                self.measure_state.first_visible_item_scroll_offset,
            )
        }
    }

    pub(crate) fn normalize_backward_jump(
        &self,
        mut index: usize,
        mut offset: f32,
    ) -> (usize, f32) {
        if offset >= 0.0 || index == 0 {
            return (index, offset);
        }

        let average_size = self.measure_state.average_item_size;

        if average_size > 0.0 && offset < -self.effective_viewport_size {
            let pixels_to_jump = (-offset) - self.effective_viewport_size;
            let items_to_jump =
                (pixels_to_jump / (average_size + self.config.spacing)).floor() as usize;

            if items_to_jump > 0 {
                let actual_jump = items_to_jump.min(index);
                if actual_jump > 0 {
                    index -= actual_jump;
                    offset += actual_jump as f32 * (average_size + self.config.spacing);
                }
            }
        }

        (index, offset)
    }

    pub(crate) fn normalize_forward_with_cache(
        &self,
        mut index: usize,
        mut offset: f32,
    ) -> (usize, f32) {
        if offset <= 0.0 {
            return (index, offset);
        }

        while index + 1 < self.items_count {
            let Some(item_size) = self.state.get_cached_size(index) else {
                break;
            };
            let leading_padding = if index == 0 {
                self.config.before_content_padding
            } else {
                0.0
            };
            let item_extent = leading_padding + item_size + self.config.spacing;
            if offset + 0.001 < item_extent {
                break;
            }
            offset -= item_extent;
            index += 1;
        }

        (index, offset)
    }

    pub(crate) fn normalize_forward(&self, mut index: usize, mut offset: f32) -> (usize, f32) {
        if offset <= 0.0 {
            return (index, offset);
        }

        let original_offset = offset;
        let leading_padding = if index == 0 {
            self.config.before_content_padding
        } else {
            0.0
        };
        if leading_padding > 0.0 {
            if offset <= leading_padding {
                return (index, offset);
            }
            offset -= leading_padding;
        }

        let average_size = self.measure_state.average_item_size;
        if average_size <= 0.0 {
            return (index, offset);
        }

        let buffer_pixels = self.effective_viewport_size;
        if offset > buffer_pixels {
            let pixels_to_skip = offset - buffer_pixels;
            let item_size_with_spacing = average_size + self.config.spacing;
            let items_to_skip = (pixels_to_skip / item_size_with_spacing).floor() as usize;

            if items_to_skip > 0 {
                let max_skip = self.items_count.saturating_sub(1).saturating_sub(index);
                let actual_skip = items_to_skip.min(max_skip);

                if actual_skip > 0 {
                    index += actual_skip;
                    offset -= actual_skip as f32 * item_size_with_spacing;
                }
            }
        }

        if index == 0 {
            offset = original_offset;
        }

        (index, offset)
    }
}

#[cfg(test)]
mod tests {
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
            let resolver = ScrollPositionResolver::new(
                &state,
                state.begin_measure_pass(),
                &config,
                100,
                500.0,
            );

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
            let resolver = ScrollPositionResolver::new(
                &state,
                state.begin_measure_pass(),
                &config,
                100,
                500.0,
            );

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
            let resolver = ScrollPositionResolver::new(
                &state,
                state.begin_measure_pass(),
                &config,
                100,
                500.0,
            );

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
            let resolver = ScrollPositionResolver::new(
                &state,
                state.begin_measure_pass(),
                &config,
                100,
                500.0,
            );

            let (index, offset) = resolver.normalize_forward(0, 1500.0);
            assert!(index > 0, "Expected forward jump, got index={}", index);
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
            let resolver = ScrollPositionResolver::new(
                &state,
                state.begin_measure_pass(),
                &config,
                100,
                500.0,
            );

            let (index, offset) = resolver.normalize_backward_jump(50, -2000.0);
            assert!(index < 50, "Expected backward jump, got index={}", index);
            assert!(offset > -2000.0, "Expected offset increase");
        });
    }
}
