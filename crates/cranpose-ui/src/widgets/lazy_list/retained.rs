// A lazy list measured from the items of its last pass: a frame that moves
// the list without bringing in or taking out an item composes nothing and
// measures no item.

use std::ops::Range;

use cranpose_core::SlotId;
use cranpose_foundation::lazy::{
    BeyondItem, LazyItemSource, LazyListIntervalContent, LazyListMeasuredItem, NeedsComposition,
    measure_lazy_list_from_retained,
};
use cranpose_ui_layout::{Axis, Constraints, MeasureResult};

use super::{
    BeyondBoundsComposition, LazyFrame, LazyItemMeasureContext, LazyListItemMeasureInputs,
    LazyListMeasureContext, beyond_composition, log_lazy_cache_telemetry, place_lazy_list,
    request_beyond_items,
};
use crate::subcompose_layout::RetainedMeasureScope;

// Measures a list as `measure_lazy_list_internal` does, from the items its
// last pass kept: `None` where an item needs a composition or a measure, and
// where that pass composes or moves the position first: for a grid, a sticky
// header, a focused item, a prefetch pass or a first item that moved.
pub(super) fn measure_lazy_list_retained(
    scope: &mut RetainedMeasureScope<'_>,
    constraints: Constraints,
    axis: Axis,
    content: &LazyListIntervalContent,
    context: &LazyListMeasureContext<'_>,
) -> Option<MeasureResult> {
    if context.config.grid.is_some()
        || crate::lazy_prefetch::in_lazy_prefetch_pass()
        || !content.header_indices().is_empty()
        || scope.focused_slot().is_some()
    {
        return None;
    }
    let frame = LazyFrame::new(constraints, axis, context.config);
    let state = context.state;
    let items_count = content.item_count();
    context.measured_item_cache.borrow_mut().begin_pass(
        frame.axis,
        frame.item_cross_axis_size,
        |slot| scope.slot_is_retained(slot),
    );
    let near = state.nearest_range();
    if items_count > 0
        && state.first_visible_item_moved(items_count, |slot_id| {
            content.get_index_by_slot_id_in_range(slot_id, near.clone())
        })
    {
        return None;
    }

    let scroll_delta = state.peek_scroll_delta();
    let mut items = RetainedListItems {
        scope,
        inputs: LazyListItemMeasureInputs {
            axis: frame.axis,
            cross_axis_size: frame.item_cross_axis_size,
            content,
            state,
            measured_item_cache: context.measured_item_cache,
        },
        near,
        scroll_delta,
        skipped_slots_checked: false,
        beyond: beyond_composition(state, scroll_delta, false),
        reused: 0,
    };
    let list = measure_lazy_list_from_retained(
        items_count,
        state,
        frame.raw_viewport_size,
        &context.config.measure,
        &mut items,
    )
    .ok()?;
    if !items.scope.kept_every_slot() {
        return None;
    }
    context
        .measured_item_cache
        .borrow_mut()
        .record_retained_reuses(items.reused);
    request_beyond_items(items.beyond, items.scope.root_id());
    let result = list.publish(state);
    log_lazy_cache_telemetry(&result, context.measured_item_cache);
    Some(place_lazy_list(
        items.scope,
        constraints,
        &result,
        items_count,
        context,
        &frame,
    ))
}

// The items a list's last pass kept, as a source that refuses an item it
// would have to compose or measure.
struct RetainedListItems<'s, 'a, 'i> {
    scope: &'s mut RetainedMeasureScope<'a>,
    inputs: LazyListItemMeasureInputs<'i>,
    near: Range<usize>,
    scroll_delta: f32,
    skipped_slots_checked: bool,
    beyond: Option<BeyondBoundsComposition>,
    // How many items the pass took as they were measured.
    reused: usize,
}

impl RetainedListItems<'_, '_, '_> {
    fn slot(&self, index: usize) -> SlotId {
        SlotId(self.inputs.content.get_key(index).to_slot_id())
    }

    // Whether a forward scroll that measures from `index` leaves behind a
    // slot of the last pass, which the composing pass recycles when it
    // measures its first item.
    fn leaves_slots_behind(&mut self, index: usize) -> bool {
        if self.skipped_slots_checked || self.scroll_delta >= -0.001 || index == 0 {
            return false;
        }
        self.skipped_slots_checked = true;
        let content = self.inputs.content;
        self.scope.unkept_slots().iter().any(|slot| {
            content
                .get_index_by_slot_id_in_range(slot.raw(), self.near.clone())
                .is_some_and(|found| found < index)
        })
    }
}

impl LazyItemSource for RetainedListItems<'_, '_, '_> {
    type Refusal = NeedsComposition;

    fn measure(&mut self, index: usize) -> Result<LazyListMeasuredItem, NeedsComposition> {
        if self.leaves_slots_behind(index) {
            return Err(NeedsComposition);
        }
        let content = self.inputs.content;
        let key_slot_id = content.get_key(index).to_slot_id();
        let content_type = content.get_content_type(index);
        if !self
            .scope
            .content_type_is(SlotId(key_slot_id), content_type)
        {
            return Err(NeedsComposition);
        }
        let cache = self.inputs.measured_item_cache.borrow();
        let cached = cache
            .candidate(&LazyItemMeasureContext {
                index,
                key_slot_id,
                content_type,
                axis: self.inputs.axis,
                cross_axis_size: self.inputs.cross_axis_size,
            })
            .ok_or(NeedsComposition)?;
        if !self
            .scope
            .keep_measured(SlotId(key_slot_id), &cached.item.roots.node_ids)
        {
            return Err(NeedsComposition);
        }
        self.reused += 1;
        Ok(cached.item.clone())
    }

    fn keep_beyond(&mut self, index: usize, distance: f32) -> Result<BeyondItem, NeedsComposition> {
        let slot = self.slot(index);
        let composed = self.scope.slot_is_active(slot);
        if !self
            .beyond
            .as_mut()
            .is_none_or(|beyond| beyond.admits(composed, distance))
        {
            return Ok(BeyondItem::Declined);
        }
        let content_type = self.inputs.content.get_content_type(index);
        if composed
            && self.scope.content_type_is(slot, content_type)
            && self.scope.keep_composed(slot, &())
        {
            Ok(BeyondItem::Kept)
        } else {
            Err(NeedsComposition)
        }
    }
}
