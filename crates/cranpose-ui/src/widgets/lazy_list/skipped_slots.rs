//! The items a pass skips: their slots go to the reuse pool before the pass
//! composes the items it shows.

use cranpose_core::SlotId;
use smallvec::SmallVec;

use super::{LazyListItemMeasureInputs, LazyListItems};
use crate::{MemoryHeadroom, subcompose_layout::SubcomposeMeasureScopeImpl};

// Before the pass of `items` measures its first item, at `first`, recycles
// the active slots of the items before it in a forward scroll, found among
// the items near the viewport, where every slot such a scroll skips was
// active. Unless the pass places the items beyond the viewport, it notes
// `first` for `recycle_out_of_reach`.
pub(super) fn recycle_skipped_active_slots(items: &mut LazyListItems<'_, '_, '_>, first: usize) {
    if items.skipped_slots_recycled {
        return;
    }
    items.skipped_slots_recycled = true;
    if !items.place_beyond {
        items.inputs.first_item.set(Some(first));
    }
    if items.scroll_delta >= -0.001 || first == 0 {
        return;
    }

    // The focused item's slot stays: the pass places it after the viewport,
    // and an item that took its node would take the focus.
    let (content, near) = (items.inputs.content, &items.near);
    let focused = items.scope.focused_slot();
    items.scope.recycle_active_slots_where(|slot_id| {
        Some(slot_id) != focused
            && content
                .get_index_by_slot_id_in_range(slot_id.raw(), near.clone())
                .is_some_and(|index| index < first)
    });
}

// Before the pass composes the item at `index`, whose slot is `slot_id` and
// whose measurement the cache did not hold: when that is the item the pass
// started at, with little memory left, and the last pass did not compose
// it, recycles the active slots of the items farther from it than the count
// of items the last pass kept. A jump then leaves the old items to the pool
// before it composes the new ones, so the two never live at once. The
// focused item's slot stays, and so does the header the pass pins over the
// first item.
pub(super) fn recycle_out_of_reach(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    inputs: &LazyListItemMeasureInputs<'_>,
    index: usize,
    slot_id: SlotId,
) {
    if inputs.first_item.take() != Some(index)
        || crate::current_memory_headroom() != MemoryHeadroom::Tight
        || scope.slot_is_active(slot_id)
    {
        return;
    }
    let content = inputs.content;
    let reach = scope.active_slots_count();
    let end = index.saturating_add(reach + 1).min(content.item_count());
    let reached: SmallVec<[SlotId; 32]> = (index.saturating_sub(reach)..end)
        .map(|item| SlotId(content.get_key(item).to_slot_id()))
        .collect();
    let headers = content.header_indices();
    let pinned = headers[..headers.partition_point(|&header| header <= index)]
        .last()
        .map(|&header| SlotId(content.get_key(header).to_slot_id()));
    let focused = scope.focused_slot();
    scope.recycle_active_slots_where(|slot| {
        Some(slot) != focused && Some(slot) != pinned && !reached.contains(&slot)
    });
}
