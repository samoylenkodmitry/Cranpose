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
#[path = "tests/scroll_position_resolver_tests.rs"]
mod tests;
