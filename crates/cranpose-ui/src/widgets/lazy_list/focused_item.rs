//! The item that holds the focus: a list measures it though it left the
//! viewport, and places it where the items between it and the viewport put it.

use cranpose_foundation::lazy::{LazyListMeasureResult, LazyListMeasuredItem};

use super::{LazyListItemMeasureInputs, measure_lazy_list_item};
use crate::subcompose_layout::SubcomposeMeasureScopeImpl;

pub(super) fn measure_focused_lazy_item(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    inputs: &LazyListItemMeasureInputs<'_>,
) -> Option<LazyListMeasuredItem> {
    let index = focused_lazy_item(scope, inputs)?;
    Some(measure_lazy_list_item(scope, index, inputs))
}

/// The index of the item that holds the focus. Focus in an item the
/// content no longer has is cleared.
pub(super) fn focused_lazy_item(
    scope: &SubcomposeMeasureScopeImpl<'_>,
    inputs: &LazyListItemMeasureInputs<'_>,
) -> Option<usize> {
    let slot = scope.focused_slot()?.raw();
    let content = inputs.content;
    let last_measured = inputs.measured_item_cache.borrow().focused_index(slot);
    let found = last_measured
        .filter(|&index| {
            index < content.item_count() && content.get_key(index).to_slot_id() == slot
        })
        .or_else(|| content.get_index_by_slot_id_in_range(slot, inputs.state.nearest_range()))
        .or_else(|| content.get_index_by_slot_id(slot));
    let Some(index) = found else {
        let _ = cranpose_core::run_in_mutable_snapshot(|| crate::FocusManager.clear_focus());
        return None;
    };
    inputs
        .measured_item_cache
        .borrow_mut()
        .remember_focused(slot, index);
    Some(index)
}

/// Places the focused `item` where the pass left it out, as far from the
/// first visible item as the items between take: `known` sizes, the first
/// visible item's size for the others.
pub(super) fn place_focused_lazy_item(
    result: &mut LazyListMeasureResult,
    mut item: LazyListMeasuredItem,
    known: impl IntoIterator<Item = (usize, f32)>,
    spacing: f32,
) {
    let Err(position) = result
        .visible_items
        .binary_search_by_key(&item.index, |entry| entry.index)
    else {
        return;
    };
    let Some(anchor) = result.visible_items.first() else {
        return;
    };
    let start = item.index.min(anchor.index);
    let end = item.index.max(anchor.index);
    let estimate = anchor.main_axis_size;
    let mut distance = (end - start) as f32 * (estimate + spacing);
    for (index, size) in known {
        if (start..end).contains(&index) {
            distance += size - estimate;
        }
    }
    item.offset = anchor.offset
        + if item.index < anchor.index {
            -distance
        } else {
            distance
        };
    result.visible_items.insert(position, item);
}
