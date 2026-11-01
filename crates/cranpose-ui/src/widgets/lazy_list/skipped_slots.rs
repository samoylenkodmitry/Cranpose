//! The items a pass skips: their slots go to the reuse pool before the pass
//! composes the items it shows.

use cranpose_foundation::lazy::LazyListIntervalContent;

use crate::subcompose_layout::SubcomposeMeasureScopeImpl;

/// Recycles the active slots of items before `first_measured_index`, found
/// among the items `near` the viewport, where every slot a forward scroll
/// skips was active.
pub(super) fn recycle_forward_skipped_active_slots(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    content: &LazyListIntervalContent,
    near: &std::ops::Range<usize>,
    first_measured_index: usize,
    scroll_delta: f32,
) -> bool {
    if scroll_delta >= -0.001 || first_measured_index == 0 {
        return false;
    }

    // The focused item's slot stays: the pass places it after the viewport,
    // and an item that took its node would take the focus.
    let focused = scope.focused_slot();
    scope.recycle_active_slots_where(|slot_id| {
        Some(slot_id) != focused
            && content
                .get_index_by_slot_id_in_range(slot_id.raw(), near.clone())
                .is_some_and(|index| index < first_measured_index)
    });
    true
}
